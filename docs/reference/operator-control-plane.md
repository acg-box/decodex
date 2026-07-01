---
type: "Reference"
title: "Operator Control Plane"
description: "Describe the current single-machine Decodex control plane, operator dashboard sections, and state ownership boundaries."
status: active
authority: current_state
owner: docs
tags: [reference]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/recovery.rs, apps/decodex/src/orchestrator/status.rs, apps/decodex/src/orchestrator/types.rs, apps/decodex/src/orchestrator/operator_http.rs, apps/decodex/src/orchestrator/operator_dashboard/body.html, apps/decodex/src/orchestrator/run_cycle.rs, apps/decodex/src/orchestrator/agent_evidence.rs, apps/decodex/src/orchestrator/tests/operator/status/http.rs, apps/decodex/src/mcp.rs]
drift_watch: [decodex serve, decodex status, decodex lane inspect, decodex recover review-handoff, decodex recover ghost-lane, decodex recover stale-active, stale_active_release, stale_active_state_restore_pending, run_stale_active_recovery, linear_active_label_present, ghost_lane_cleanup_audit_present, mcp_test_fixture_ghost_lane, decodex evidence, decodex mcp serve --transport stdio, decodex mcp serve --transport streamable-http, phase_acceptance_check, control_plane_snapshot, operator dashboard, runtime.sqlite3, project.toml, WORKFLOW.md]
last_verified: 2026-06-30
---
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
- The everyday loop-runtime surface remains Codex conversation. Research/decision
  promotion and internal Execution Program state are governed by
  [`../spec/loop-runtime.md`](../spec/loop-runtime.md), not by dashboard graph editing
  or user-visible DAG commands.
- Program intake readback is intentionally summarized: operators may see counts of
  ready, dispatchable, blocked, held, stale, attention, completed, and mapped issue
  identifiers. Optional planned, mapped, active, and superseded counts may appear when
  the runtime has that detail. Low-level node edges and graph operations remain
  internal runtime state. Status JSON may include sparse node readbacks for direct
  dispatch decisions and held, blocked, stale, active, or attention-bound nodes:
  mapped issue identifier, issue state, lifecycle/readiness state, dispatch action,
  public-safe reason codes, public-safe reasons, and a recovery next action. It must not expose
  Decision Contract payloads, raw graph edges, local paths, credentials, raw logs, or
  private runtime events.
- Autonomy readback is status-derived, not SQLite-inspection-only. Running lane status
  may include the accepted Objective Contract id/version, recent signals with public
  source refs and freshness, proposal states and refusal reasons, public-safe proposal
  -> Decision Contract -> Program Intake lineage, and report metadata with source
  refs, redaction level, completeness, and known gaps. The browser dashboard and
  Decodex App must display autonomy progress or freshness only when the published
  runtime status includes fresh signal source refs; otherwise they show that source
  refs are needed. Report rows are labeled as derived query views and are not audit
  authority.
- Persisted Execution Programs are evaluated by the Program scheduler before ordinary
  queued-label issue selection. Ready, startable mapped nodes can be dispatched
  directly with `program` dispatch mode; blocked, stale, paused, terminal,
  attention-required, active, or conflict-held nodes stay held. Service queue labels
  are not applied, removed, retained, or treated as Program ownership evidence.

Decodex App is a native shell over the same local runtime and account-pool state. On
launch it connects to an existing default local listener when one is reachable; if
not, it starts the bundled `decodex` binary as
`decodex serve --listen-address 127.0.0.1:8192`. The app
fallback is a normal control-plane server: it loads the registered project registry,
schedules only enabled projects, keeps active runtime DB-backed runs visible even when
a project is disabled
for future dispatch, uses the CLI-owned default cadences, and serves the dashboard,
account APIs, `GET /api/operator-snapshot`,
`POST /api/linear-scan`, `GET /api/lane/inspect`, and `POST /api/lane/interrupt` from
the single local listener.
The default listener must have exactly one owner. When Decodex App has started its
bundled helper on `127.0.0.1:8192`, do not also keep a standalone launchd job such as
`space.decodex.serve` or another `decodex serve --listen-address 127.0.0.1:8192`
process running; that duplicate owner will repeatedly fail with `Address already in use`
and should be removed or pointed at a different explicit listen address.
Use `decodex app` to open the installed macOS app from the CLI; use
`decodex app --bundle <APP_BUNDLE> --new` for a staged app bundle. The launch preserves
the caller's environment, so `DECODEX_APP_SERVER_URL` remains an explicit App preview
override when set.

Local MCP hosts can use `decodex mcp serve --transport stdio` for local core MCP
protocol primitives or `decodex mcp serve --transport streamable-http` for remote
permitted clients that reach the daemon through an operator-chosen local listener,
tunnel, or relay. Both transports list and read checked-in docs, checked-in Markdown
research concepts, Decision Contract readback, status snapshots, and lane-control
readback; both also advertise resource templates for `status_live`, `activity_tail`,
`lane_inspect`, current/recent status-window run events, protocol activity,
child-agent activity, progress diagnostics, and PR/review state. These projections
reuse local operator snapshot summaries and exclude hidden reasoning, raw steer text,
private evidence, and local path payloads. Run-scoped resource reads return
`resource_not_found` for run ids outside the current/recent status snapshot rather
than constructing an unbounded historical snapshot. Both transports advertise reusable Decodex prompts and a small
schema-bound tool catalog. The stdio gateway defaults to
`--capability-profile admin` for local clients. Streamable HTTP binds to
`127.0.0.1:8193` and defaults to `observe`; it serves JSON-RPC at `POST /mcp`,
validates browser `Origin` headers against loopback or `--allow-origin`, issues
`Mcp-Session-Id` on `initialize`, requires a known session after initialization, and
uses SSE framing for progress or notifications when the client accepts
`text/event-stream`. The MCP session is not authorization, and `--allow-origin` is
not authentication. Remote Streamable HTTP beyond loopback requires both a trusted
origin and `--bearer-token-env`; any Streamable HTTP profile above `observe` also
requires `--bearer-token-env`. Decodex validates the HTTP `Authorization: Bearer`
header for non-preflight requests when that boundary is configured. This bearer guard
is a direct-listener boundary, not OAuth Protected Resource Metadata, so operators may
still prefer an OAuth-capable relay, tunnel, reverse proxy, or network ACL. Both
transports can be
narrowed or explicitly elevated with
`--capability-profile observe|plan|operate|admin`; `tools/list` filters by the active
profile and above-profile calls return structured refusals. Observe and plan tools are
read-oriented. Operate exposes `decodex_lane_control` as an inspect-first facade:
`inspect` returns current lane-control preconditions, `steer` and `interrupt` require
matching inspected run/turn authority, and unsupported shortcut paths refuse to the
canonical tracker/runtime lifecycle. Admin exposes `decodex_project_control` for
project status plus future-dispatch-only pause/resume with explicit authority; scan
requests refuse to the operator control loop.

`decodex serve` has two hardcoded scheduler cadences:

- The local control-plane loop publishes operator snapshots every 15 seconds.
- Linear-backed queue/status scans run at most every 5 minutes per project, unless
  an operator or agent queues an explicit scan request with
  `POST /api/linear-scan`.

Agents that just created or relabeled queue issues can avoid waiting for the next
5-minute Linear poll by sending a targeted local request:

```sh
curl -sS -X POST http://127.0.0.1:8192/api/linear-scan \
  -H 'Content-Type: application/json' \
  -d '{"projectId":"decodex"}'
```

An empty `POST /api/linear-scan` queues a scan for all enabled projects. Requests are
consumed by the next 15-second control-plane tick and still respect any active tracker
rate-limit backoff.

Research/design work has a runtime-local command path:

```sh
decodex research compile --intent "research X"
decodex research compile --input research-design-run.json
decodex research promote <CONTRACT_ID>
```

`research compile` writes a local Decision Contract candidate into runtime SQLite and
returns a bounded outcome: `decision_ready`, `not_decision_ready`, `blocked`, or
`needs_human_decision`. Decodex research first probes the decision, records evidence,
compares options, forms a challenge-ready judgment, resolves or preserves challenge
objections, and only then chooses the outcome. It may retain private evidence
references, option comparisons, structured proposed issues, conflict domains, and
dispatch intent for later issue shaping, but it does not enqueue issues, mutate Linear
state, set goals, or start lane execution. `research promote` records the accepted
boundary for an existing contract; later issue generation or Execution Program
readiness must consume the promoted contract's structured `proposed_issues[]` instead
of treating a research summary as authority.

Lane inspect and interrupt are local control APIs, not dashboard UI actions:

```sh
curl -sS 'http://127.0.0.1:8192/api/lane/inspect?projectId=decodex&issue=XY-703'
curl -sS -X POST http://127.0.0.1:8192/api/lane/interrupt \
  -H 'Content-Type: application/json' \
  -d '{"projectId":"decodex","issue":"XY-703","runId":"<run-id>"}'
```

`POST /api/lane/interrupt` first writes a soft interrupt request for the active
app-server child to deliver with `turn/interrupt`. Add `"force": true` only when the
operator explicitly wants hard process-kill fallback after soft interrupt is
unavailable, does not return in the local wait window, or is rejected with
`run_lease_missing` while the same lane still has recorded live process evidence.
Hard fallback is reported as `hard_interrupt_fallback`, not as a graceful stop. If no
signalable child process is recorded, the force response remains a recovery/inspection
result and must not be read as a successful soft interrupt.

Use `--dev` only for isolated local development:

- Developers may use `--dev` to exercise real account APIs, `GET /api/operator-snapshot`,
  and dashboard routes against local runtime state without starting automation.
- Do not use `--dev` for operator automation, queue intake, retained-lane recovery,
  project registration refresh, or service scheduling. It is hidden from CLI help and
  intentionally rejects `--config`; `serve` has no interval override argument.
- For browser dashboard and Decodex App preview UI work, use one
  `dev/operator-dashboard-mock.mjs` listener instead of `--dev`. The same mock base
  URL must serve the browser dashboard, `/api/accounts`, and the Decodex App
  dashboard WebSocket connection; do not start a separate App mock server. When
  `DECODEX_APP_SERVER_URL` is set, Decodex App treats that URL as authoritative and
  does not fall back to the default `127.0.0.1:8192` runtime.

Project registration is not service intake. The `Projects` dashboard section may show
multiple enabled projects with visible work at once, and its filter can reveal the full
registered-project table, but a service is only eligible to intake
Linear issues labeled with its matching `decodex:queued:<service-id>` label. For
example, a Decodex-only run intakes issues labeled `decodex:queued:decodex`; `rsnap`
can stay enabled in the full project registry, and issues labeled `decodex:queued:rsnap`
remain rsnap intake rather than Decodex intake. Operators may still enqueue normal
issues manually with service-scoped queue labels, but persisted Execution Programs
dispatch ready mapped nodes directly and do not mutate those labels.

When `decodex run --dry-run` or the status output has no eligible intake candidate,
the operator hint points to the ordinary queue checklist: `Todo`, the service-scoped
`decodex:queued:<service-id>` label, no opt-out or needs-attention labels, a
non-terminal state, no open dependency blockers, and no active issue claim. Ready
Program Intake nodes are not queue-label candidates; they appear under Execution
Programs, and `decodex run <ISSUE>` can start a mapped dispatchable Program node with
`program` dispatch mode.

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
When a run has harness-outcome telemetry, the evidence readback also summarizes
candidate improvements by kind, reason code, target, source-event count, and
recommendation. These summaries are local operator guidance; they are not Linear
ledger records and do not automatically edit prompts, skills, validators, issue
templates, or loop policy.
The same private-evidence readback exposes compact review checkpoint, phase
acceptance, architecture recovery, and authority-boundary summaries for the selected
run/attempt: review phase, status, head, compatibility round, review cost class, risk
class, compact eligibility, fallback reason, active/stop finding fingerprints,
finding counts; phase acceptance decision, reason, objective coverage, effective
delta, changed surfaces, non-goal result, validation result, and next action;
recovery reason, boundary disposition, budget; and boundary disposition, reason,
attempted recovery, changed-surface count, and improvement-signal count. These
summaries are safe operator readback; compact review is not skipped review, and raw
reviewer finding bodies, checkpoint payloads, changed-surface payloads, retained
diffs, logs, and transcripts remain hidden unless `--include-payload` is explicitly
requested for local repair.
Private phase-goal evidence may also include `phase_goal_recovery`. That event means
Decodex found a still-active implementation or repair phase goal after an app-server
failure or child exit, ran the registered repo gate itself, persisted the next phase,
and scheduled continuation instead of writing `decodex:needs-attention`. It is a
runtime recovery handoff, not final issue success; the later `handoff_evidence` phase
still owns ordinary review, push, PR creation, and terminal finalize, while
`review_repair_evidence` owns retained repaired-head push, PR readback, repair
completion intent, and `review_repair` terminal finalize.
Private phase-goal evidence may also include `phase_acceptance_check`. That event
records why Decodex allowed an implementation or repair goal to advance after repo
gate validation, or why it kept the lane in repair even though validation passed. The
operator status summary may show the latest acceptance decision and next action; the
runtime SQLite row remains the authoritative local evidence.
Retry comments with `phase_goal_terminal_path_missing` mean a phase goal reached
`complete` before the required Decodex terminal tool path was recorded. The lane is
still runtime-owned while retry budget remains; the next attempt re-enters the
persisted phase and must record review handoff, closeout, or manual attention before
the issue can leave automation ownership.
Retry comments with `app_server_transport_disconnected` during `initialize`,
`account/login/start`, `thread/start`, or `thread/resume` mean Decodex is restarting
the app-server under the retry budget, not asking for operator attention yet. The
same error class becomes actionable only after retry exhaustion or when the disconnect
occurred after a thread session was attached.
Retry comments with `app_server_usage_limit_exceeded` mean the active Codex account
hit a capacity limit and Decodex will re-run account selection on the next attempt.
They are actionable only after retry exhaustion or when the operator intentionally
pins all new runs to an exhausted fixed account.

## State Ownership

| Surface | Owns | Does Not Own |
| --- | --- | --- |
| Runtime SQLite DB | run leases, attempts, run-control channels, protocol events, private execution events, Decision Contracts, Program Intake Plans, internal Execution Programs, dispatch readiness, worktree mappings, retry state, retained PR state, review-policy checkpoints with structured independent-review detail, loop-guardrail checkpoints, phase-goal signals, phase timing, connector backoff, project registry | human backlog grooming or durable team-visible issue history |
| Central project config | `service_id`, repo root, worktree root, tracker/GitHub credential env-var names, enabled project registration | per-run state or issue ownership |
| Project `WORKFLOW.md` | repo policy, validation gate, state names, retry/review policy | runtime ownership, queue labels, credentials, model overrides |
| Linear | team-visible issue state, queue/active/manual-attention labels, coarse execution ledger comments, progress/failure/handoff/closeout summaries | high-frequency runtime truth, heartbeat, token pressure, raw attempts, private execution evidence, connector retry budgets |
| GitHub | PR, checks, review comments, merge evidence, signed commit verification | queue selection or local lane ownership |
| `.decodex-run-activity` | short-lived child activity heartbeat for the active attempt, including same-boot and same-process-start liveness plus diagnostic protocol/child/account breadcrumbs | durable ownership, review handoff identity, review-policy checkpoint authority, cleanup authority |
| `.decodex-run-control/` | local per-attempt control-channel marker files for active runtime-owned attempts | standalone ownership proof, public tracker history, or dashboard-authored lane mutation |

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
section says whether the path belongs to a run lease, retained review/landing
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
The `Work.waiting` project number is a blocked/deferred-work summary: retry backoff,
continuation waits, explicit external waits, operator or user-input waits, protocol
idleness, queued waiting candidates, and unshadowed wait-for-review lanes. Normal
fresh model, tool, and repo-gate execution remains running work even when the lane row
shows detailed wait/tone diagnostics.

The browser dashboard and Decodex App read the complete published operator state from
the local `GET /dashboard/control` WebSocket. That socket is the dashboard/App
authority for published snapshots, current-lane activity updates, and local dashboard
control acknowledgements. The App still uses the same base URL's `/api/accounts`
HTTP surface for account-pool rows and actions. `GET /api/operator-snapshot` is a
status/cache read path over the same runtime database, not a browser-dashboard or App
polling authority and not a sign that the dev listener owns scheduling.

`decodex status` uses that same local `GET /api/operator-snapshot` as a fast read path
when the default listener is reachable, the published snapshot is recent, includes the
requested project, and its run limit is large enough for the requested status limit.
The CLI projects aggregate snapshots down to the requested project before printing.
JSON output identifies cache hits with `"status_source": "operator_snapshot_cache"` and
`snapshot_age_seconds`. If the snapshot is missing, stale, mismatched, or too small,
the command falls back to a direct local runtime read and emits
`status_cached_snapshot_unavailable` in `warning_details`. `decodex status --live`
always bypasses the cached snapshot and rebuilds status with fresh Linear/GitHub
observers. If a downstream consumer such as `head` closes stdout before status output
finishes, the CLI treats that broken pipe as normal truncated output rather than an
operator status failure.

Operator JSON snapshots include `execution_programs[]` for Program Intake and
Execution Program readback. Each program row carries public intake kind/summary,
source contract id when present, the compact summary counts, mapped issue
identifiers, dispatchable count, and sparse `node_readbacks[]` for direct dispatch
decisions or nodes that need operator context. Live status refreshes mapped issue
state, service labels, needs-attention labels, dependency blockers, and post-review
lifecycle ownership before it evaluates those program counts, so terminal Linear
states and cleared labels supersede older persisted Program mappings. It also reads
local shared run claims; a mapped issue with a live lease is shown as an active
Program node rather than a cleanup-only conflict-domain blocker. Dependency
diagnostics use `dependency_not_terminal` with a next action to complete the
dependency issue or refresh the Execution Program dependency plan when a stale
dependency program is the real blocker.

For the lane-control rollout, current-lane UI posture is observe-only. The dashboard
renders current-lane state, protocol activity, liveness, private-evidence references,
local run-control capability metadata, and local acknowledgement/account controls, but
it is not the supported place to author
steer, retry, task replacement, or lifecycle mutations. CLI/API is the first
operator-control surface for lane control, governed by
[`../spec/lane-control.md`](../spec/lane-control.md). The browser UI does not show or
accept current-lane stop/interrupt controls, project pause/resume controls, manual retry
controls, or current-lane steer controls; use `decodex lane inspect`, `decodex lane
interrupt`, or the local `/api/lane/*` endpoints instead. Account-pool selection remains available
because it changes the global Codex account selector, not a current lane.
Active-lane steer is available through `decodex lane steer <ISSUE> --run-id <RUN_ID>
--expected-turn-id <TURN_ID> --message <TEXT>`, canonical `POST /api/lane/steer`,
and legacy alias `POST /api/lane-steer`. These surfaces require the expected active
turn id, audit accepted or rejected state locally, and keep raw steer text out of
public tracker projections.
`runActivity.currentLanesComplete`
marks whether a payload is the complete current-lane list; subscription-filtered
payloads set it to `false`, so consumers must not treat a missing run in that payload
as ended.
Current lane rows may include `control_capability` with the active attempt's project,
issue, run id, attempt, current thread/turn ids, local transport, channel path, status,
and timestamps. It is local routing metadata for CLI/API controls, not a dashboard
command surface.
After a steer request is handled, current lane protocol activity may show a compact
`turn/steer` entry with outcome, failure class, and response turn id. It does not
include the operator message.
MCP lane-control resources remain readback only and mirror the local status/inspect
projection. Remote-control-capable MCP clients request lane-control actions through the
profile-gated `decodex_lane_control` tool, which uses the same inspect-first
preconditions, run/turn authority, local audit records, and structured refusals as the
supported `decodex lane inspect`, `decodex lane interrupt`, `decodex lane steer`, and
local `/api/lane/*` endpoints.
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
| `Accounts` | Shared Codex account pool and usage table from `~/.codex/decodex/accounts.jsonl` when `[codex.accounts]` is enabled for a project. Account identity can be obscured from the `Account` column header eye without changing the underlying snapshot. The row weight column shows the capacity multiplier used for pool usage estimates: `pro` accounts count as `20x`, and all other plans count as `1x`. Usage probes read Codex `/wham/usage` for window capacity and `/wham/profiles/me` for profile token stats such as lifetime tokens, peak daily tokens, longest task, streaks, and daily token activity. Refresh authentication failures are persisted as `auth_failed` on the matching account, excluded from later selection, and surfaced with login recovery rather than retry-probe recovery. Selecting an account writes the global `[codex.accounts].fixed_account` selector in `~/.codex/decodex/config.toml`; clearing it returns all new account-pool runs to balanced account selection. Account display-name rerolls write `[codex.account_names.offsets]` in the same global config so Decodex App and the dashboard share the privacy-preserving names. Theme, sort, and identity-visibility preferences are client-local presentation state. The selector is global and does not pin a project to an account. |
| `Projects` | Fleet-level project table. The section-level filter toggles between active project work and the full registry. Location is its own compact path column and can be obscured from the location header eye. `Activity` shows a relative timestamp or `-`; `Work` is `running/waiting/attention`. Its waiting number summarizes blocked/deferred work and should not duplicate per-lane diagnostics already shown below. |
| `Running Lanes` | Active leased or live-executing issue lanes. A lane here is currently owned by this local control plane, or a live process/thread/protocol marker still explains active execution even when the queue lease is not held. It shows issue identity, phase, operation, attempt, queue lease state, execution liveness, thread/protocol status, child-agent activity when captured, phase-goal status when app-server reports it, timing, branch, and worktree. For the same run/attempt, newer marker protocol summary supersedes stale durable event readback so current tool/model activity is not hidden behind older maintenance events. |
| `Program Intake` | Read-only Program Intake and Execution Program progress. It shows each active program's public intake summary, status, mapped issue identifiers, summary counts, dispatchable counts, and sparse node diagnostics for dispatch decisions or held/blocked/stale/attention nodes. It does not expose graph editing, raw node-edge mutation controls, Decision Contract payloads, or private runtime evidence. |
| `Intake Queue` | Queued tracker issues before execution. Candidates are classified as `ready`, claimed without a matching local lane, blocked, or closed/stale. Repeated identical open dependency blockers surface as `dependency_program_stale` after the guardrail threshold so operators can distinguish a stale Execution Program/dependency plan from a newly blocked queue item. A queued candidate whose retained worktree already has a matching review lifecycle record is blocked with `review_handoff_state_transition_pending`; post-review recovery, review repair, landing, or closeout owns the next step instead of ordinary intake. A blocked queued candidate can still show an attached `.worktrees/XY-*` path when the queue owns the attention state; if that worktree has tracked changes after stalled reconciliation, failure writeback, or retries, the candidate is partial retained progress and not just a generic stalled or retry-budget hold. If the worktree is clean but stale active ownership remains after failed-start retry accounting, the candidate is failed-start cleanup debt rather than retained partial progress only when private evidence has no open issue-level phase continuation such as `handoff_evidence`. Human-required authority stops expose their compact decision request fields here: `phase = human_required`, reason, boundary, `decision_request_id`, and `next_action`. When queued attention still maps to a run/attempt, it also carries the same compact loop status used by running lanes. Running lanes are not repeated as normal intake work. |
| `Review & Landing` | Retained PR lanes after review handoff. This section owns post-review repair, wait-for-review, ready-to-land, closeout, cleanup, and blocked retained-lane visibility. Retained lanes expose compact loop status for their bound handoff run/attempt so operators can see review repair checkpoint state, architecture recovery stops, and boundary/human-required disposition without direct SQLite inspection. |
| `Recovery Worktrees` | Retained local worktrees that are not currently owned by `Running Lanes`, `Review & Landing`, or queued attention in `Intake Queue`. This is the cleanup or recovery inbox for recovered paths, retained PR leftovers, and cleanup-only local worktrees. Empty is the normal healthy state. Terminal unleased runtime-recorded mappings with identifier-style ids and missing checkout paths are local terminal residue, not recovery worktrees; snapshots omit them from this count and emit `stale_terminal_local_worktree_mapping_ignored` warning detail instead of refreshing those ids through Linear. |
| `Run Ledger` | Completed or non-running issue history, grouped by issue/lane. Decodex Linear execution ledger comments provide the durable completed outcome when available. If no `decodex.linear_execution_event` record exists, the row reports `missing` / `execution_ledger_missing`; the control plane does not derive a completed or landed outcome from tracker state, local attempts, or non-ledger comments. Terminal unleased local residue with identifier-style ids reports `local_terminal_residue` because Linear ledger lookup is intentionally skipped for ids that are not proven Linear issue ids. Terminal attention rows are history unless a current attention signal still exists. Raw local attempts and heartbeat details stay in debug expansion. |

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
4. For authority-boundary stops, inspect `decision_requests` for the public-safe
   decision request id, boundary, recommendation, resume condition, and next action,
   then use the linked Authority Boundary Check summary to audit the stop.
5. For Decodex Review or architecture recovery stops, inspect `review_checkpoints`,
   `architecture_recoveries`, and `boundary_checks` before deciding whether the lane
   is still autonomous, exhausted, or human-required.
6. Use `--include-payload` only when compact payload summaries are insufficient for
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
The same boundary applies to Decision Contracts: the operator surface may show status,
readiness summary, generated issue links, or public projection references, but the
versioned contract payload and private evidence references remain runtime-local.
Authority decision requests follow the same split: Linear and status show the
decision interface and resume condition, while retained worktree evidence, diff
evidence, recovery context, and boundary-check links remain local runtime evidence.
Harness-outcome telemetry follows the same rule: the operator surface may show compact
improvement-candidate summaries, while the correlated source intent, contract payloads,
review details, validation diagnostics, and guardrail checkpoints remain private
runtime evidence unless explicitly read through the local evidence command.

Worktree visibility follows the owning dashboard section:

- `Running Lanes` means the runtime DB still has a run lease, active attempt, or
  child process/thread/protocol relationship for the path. Process liveness requires
  an alive PID plus matching `.decodex-run-activity` `host_boot_id` and
  `process_start_identity`; a previous-boot marker, same-boot PID reuse, missing
  identity, an unreaped zombie PID, or unavailable current host/process identity is
  recovery input, not proof of active execution. `execution_liveness =
  process_identity_mismatch` is the stable summary for previous-boot or PID-reuse
  evidence, while `process_liveness_reason` explains the exact failed identity check
  when `process_alive` is false.
  `run_lease` is queue lease ownership only; `execution_liveness` explains why
  the lane is still visible when the queue lease is not held.
  If a newer visible attempt for the same issue exists, older attempts with stale
  protocol/process evidence are shadowed out of `Running Lanes` and current-attention
  counts so one issue does not appear as simultaneous current work or rebound after
  the newer attempt releases its lease.
  Retry, Program, and automatic continuation attempts also read the issue's latest
  unterminated phase-goal state before the current attempt, so a lane already in
  `handoff_evidence` or `review_repair_evidence` resumes the terminal evidence phase
  instead of repeating implementation just because the attempt identity changed. Empty
  failed-start attempts do not reset that phase cursor; terminal finalization, review
  completion, decision requests, blocked recovery, blocker checkpoints, or an audited
  failed-start cleanup do.
- Lane steer and interrupt rejections such as `run_lease_missing` are private
  runtime evidence. They should preserve the queue lease state, branch, retained
  worktree path, current run id and attempt, active channel metadata, and observed
  process/protocol liveness so operators can decide between wait, force interrupt,
  retained resume, or manual attention without reconstructing state from local paths.
- In the JSON snapshot, `current_lane_count` follows the same visibility boundary as
  the top-level `current_lanes` list. The `Projects` table's `running` work number uses
  `running_lane_count`, so stopped, stale, or attention lanes can stay visible as
  active work without being counted as currently running.
- Running lanes derive CLI and dashboard text from the same `OperatorRunStatus`
  object. MCP observability resources expose the same public subset as JSON
  projections for remote-safe clients: current operation, phase, event counts,
  protocol activity, child-agent activity, progress diagnostics, lane-control next
  action, and PR/review state for lanes in the current/recent status snapshot.
  `protocol_activity`, when present, summarizes app-server structured
  notifications for turn status, waiting reason, and recent protocol events. The
  dashboard uses that shared summary to explain whether active time is going to model
  execution, tools, approval/user input, or protocol idleness. Account usage details
  stay in the `Accounts` table; connector rate-limit backoff is surfaced as project
  and snapshot health, not repeated in each lane debug row. These high-frequency
  details remain local/operator-only and are not written to Linear except through
  existing lifecycle summaries. `last_protocol_activity_at` may move for any incoming
  protocol event, but `last_progress_at` moves only for meaningful work events. If a
  running lane remains in model execution with fresh account, rate-limit, phase-goal,
  or passive status traffic but stale work progress, status JSON exposes
  `progress_diagnostic = "protocol_only_activity"` so operators can separate
  process-alive and protocol-active from work-progressing.
- Running lane lifecycle metrics preserve where each attempt came from. Recorded
  runtime attempts count as `recorded`, recovered local evidence counts as
  `recovered`, and the live row's current projection counts as `current_snapshot`.
  The JSON snapshot and dashboard debug details expose per-attempt evidence and
  recovery gaps so an operator or agent can see whether a lifecycle phase came from a
  run attempt row, run lease, run-control channel, protocol summary, private execution
  event, review checkpoint, activity summary, or worktree marker. Those details are
  local/operator-only diagnostics: weak private or review evidence may explain an
  issue's lifecycle history, but it does not by itself make that issue visible as a
  current running lane.
- Status JSON and the dashboard share a `loop_status` object when a row can be tied
  to a runtime run/attempt. It carries `review_level`, `autonomy`, concise `summary`
  and `next_action`, plus optional `review`, `architecture_recovery`, `boundary`, and
  `decision_request` subobjects. Text status renders the same state as `loop_status`,
  `loop_review`, `loop_architecture_recovery`, and `loop_boundary` lines so operators
  can answer what the lane is doing and why without knowing runtime event names.
- Dynamic tool failures appear in local protocol activity as
  `item/tool/call/failure` with a normalized failure class and next action. Invalid
  or undeclared app-server tool requests are protocol failures; declared Decodex
  tools that return `success = false` remain tool failures the model can correct
  within the same turn.
- Phase-goal protocol activity may appear as `thread/goal/set`,
  `thread/goal/get`, `thread/goal/updated`, or `thread/goal/clear`. These events
  help explain whether a retained lane is implementing, repairing validation,
  repairing accepted review findings, or preparing handoff evidence. Retained lanes
  require this goal-method support; missing goal methods surface as an unsupported
  app-server blocker rather than ordinary continuation. Goal status is diagnostic
  phase evidence only; it is not a Run Ledger outcome and does not replace repo
  validation, bounded review, PR handoff, manual attention, landing, closeout, or
  terminal finalization.
- Loop guardrail stops may appear as `validation_repeat`, `no_effective_diff`,
  `remaining_delta_unchanged`, `review_churn`, `dependency_program_stale`,
  `uncovered_direction`, or `ambiguous_retained_progress`. Use the public reason to
  choose the recovery path, then inspect `decodex evidence` or local status for
  `loop_guardrail_checkpoint`, Authority Boundary policy decision, enhanced-evidence,
  and landing-block evidence before clearing attention labels or retrying.
- `Review & Landing` means a retained PR lane still owns the path for review repair,
  landing, closeout, or retained-lane cleanup.
- `missing_review_handoff_record` in `Review & Landing` means Decodex found a retained
  review worktree but cannot find the authoritative runtime DB review lifecycle
  record. Treat this as an orphaned retained review lane: inspect it with
  `decodex recover review-handoff diagnose <ISSUE>`, then use the explicit rebind path
  only after the PR URL and retained worktree lineage match exactly.
- Review lifecycle handoff or phase head mismatch reasons mean Decodex found a
  retained lifecycle record but one stored field no longer matches the clean retained
  worktree and PR head. `decodex status` keeps the bound PR URL visible when it can
  identify the lifecycle record, and `decodex recover review-handoff diagnose <ISSUE>`
  reports the stored handoff head, phase head, PR head, and mismatched field before
  any explicit rebind refresh.
- `review_handoff_ownership_drift` means the retained lifecycle record is bound but
  the active service label is missing. If the same-PR same-head lane is still in
  progress state or has drifted back to the workflow failure state, diagnosis points to
  `decodex recover review-handoff rebind --dry-run`; live rebind can restore the
  active service label only after proving retained worktree and PR lineage. Bound
  success-state lanes may still ask for ownership confirmation before the existing
  post-review lifecycle continues.
- `ownership_state = ghost_lane` with
  `policy_state = runtime_recovery_required` in live observer status or a fresh
  daemon-cached status means a local current lane still has a run lease, but tracker
  readback proves the issue entity is missing and local inspection found no retained
  worktree, ordinary control-channel/private evidence, live process signal, PR
  lineage, or review lifecycle row. The supported
  next action is `decodex recover ghost-lane diagnose <ISSUE> --json`, followed by
  `decodex recover ghost-lane cleanup <ISSUE> --dry-run` and then the non-dry-run
  cleanup only if the diagnostic remains safe. This path writes local private audit
  evidence, marks the attempt `terminal_guarded`, and clears the local lease; it does
  not mutate Linear when the issue is missing. The no-cache local-runtime status
  fallback does not prove tracker absence by itself; use `--live` or the recovery
  diagnostic when that proof matters.
- `classification = mcp_test_fixture_ghost_lane` in `recover ghost-lane diagnose`
  means Decodex matched the narrow historical PubFi MCP fixture lane: exact
  `PUB-012` / `run-12` attempt 1, optional `thread-12` / `turn-12`, missing tracker
  issue, missing worktree, missing control-channel file, no PR/review lineage, and
  private evidence made only of `source = mcp-test` lane-control request events plus
  `control_action` audit rows whose `source` is `mcp-test` or fixture-matching `cli`.
  This classification explains why stale control-channel row, thread/protocol
  summary, and private evidence conditions can still be cleanup-safe for that
  fixture. It is not a general private-evidence bypass.
- `ghost_lane_cleanup_audit_present` means a prior supported cleanup wrote a local
  `ghost_lane_cleanup` private audit with `cleared_run_lease = true`, no blockers,
  and evidence for missing tracker issue, missing worktree, and missing review
  lineage. Status and diagnose treat that audit as idempotent recovery evidence:
  when no retained worktree, live process, PR lineage, review lifecycle, or mixed
  private evidence remains, the row is history-only and must not inflate current lane
  or retained-attention counts.
- Retained worktree and post-review scans isolate stale local issue identifiers when
  tracker refresh reports a missing or invalid issue. Status and dry-run candidate
  selection may drop that stale local row, but the stale row must not hide unrelated
  valid retained project issues or fail the registered project readback.
- `policy_state = runtime_recovery_blocked` on the same tracker-backed status surfaces
  means the issue is missing from tracker readback but at least one fail-closed
  blocker exists, such as retained worktree, control-channel file, live execution
  evidence, private evidence outside the allowed PubFi MCP fixture control rows,
  mixed private evidence, PR lineage, or review lifecycle state. Preserve attention
  and inspect the named blocker. Do not use review-handoff recovery, review-checkpoint
  writeback, label cleanup, or raw SQLite edits for this state.
- `pull_request_state_read_failed` in `Review & Landing` is a degraded PR readback
  warning when the retained review lifecycle record still exists. `decodex status`
  must keep the issue identifier, branch, lifecycle PR URL, and lifecycle PR head SHA
  visible so operators can retry status, inspect the PR directly, or run the explicit
  recovery path without losing the bound PR identity. Local status JSON, text output, and
  dashboard readback also carry `readback_root_cause` when Decodex can classify the
  local diagnostic safely, for example `missing_github_cli`, `missing_github_token`,
  `github_auth_failed`, `github_api_read_failed`, `github_response_parse_failed`,
  `pull_request_shape_read_failed`, or `lineage_validation_failed`. This warning is a
  wait/retry lane, not passive manual attention, unless the post-review classification
  decision itself is `Block`. These diagnostic
  tokens are operator-local and must not include tokens, raw API payloads, or private
  command output.
- `worktree_checkout_branch_read_failed` and `worktree_head_read_failed` in
  `Review & Landing` are degraded local worktree readbacks for a still-bound retained
  lane. They may block a fresh classification for this status tick, but they must stay
  wait/retry readback conditions and must not add `decodex:needs-attention` unless a
  later successful readback proves a hard blocker such as a missing branch, branch
  mismatch, missing head, or lineage mismatch.
- `pull_request_merge_state_conflict` in `Review & Landing` means one retained
  post-review readback looked merge-complete but direct PR merge readback did not
  confirm that the same PR head is merged. Treat it as a readback contradiction, not a
  closeout-ready lane: inspect the PR directly and retry status after the GitHub state
  is consistent.
- `Intake Queue` means queued attention still owns the path, including partial retained
  progress after retries.
- `dependency_program_stale` in `Intake Queue` means the same open blocker fingerprint
  has repeated through the guardrail threshold. Refresh the dependency issue, split or
  repair the Execution Program, or route research/decision work before requeueing; do
  not clear it as a transient queue delay.
- `linear_active_label_present` in `Intake Queue` means the issue still carries
  service active ownership while it is also queued, but local status could not prove a
  matching run lease. Treat it as a recovery/attention row, not ready work. If its
  attention cause is `evidence_missing` and the worktree has no tracked changes,
  status sets `attention_next_action = run_stale_active_recovery` and points to
  `decodex recover stale-active diagnose <ISSUE>` followed by
  `decodex recover stale-active release <ISSUE> --dry-run`. The release command
  preserves any queue label, treats a run lease or active shared claim as recoverable
  only when the marker run id and attempt match the latest leased run, the local
  lease belongs to that same project/run, and no external or incompatible shared
  claim is present, treats dead-process runtime telemetry such as
  implementation phase-goal recovery rows, app-server no-diff loop guardrail
  checkpoints, and no-progress harness outcomes as recoverable only after process
  identity proves the recorded child is gone and
  worktree/branch/private/lineage checks are clean, blocks on review-policy
  checkpoints and issue-id or issue-identifier PR lineage, reads local runtime
  evidence under both issue id keys, terminalizes stale local ownership as
  `terminal_guarded`, clears only matching proven-dead local run leases, writes a
  local private `stale_active_release` audit when a stale run attempt exists, repeats
  the run-lease/shared-claim guard, rechecks tracker labels, and removes only the
  service active label as the final mutation. If a
  retained worktree has tracked changes, untracked non-runtime files, unmerged local
  commits, unavailable default-branch proof, or cannot be inspected, status uses
  `inspect_retained_worktree_changes_before_stale_active_recovery` instead.
  If a previous release attempt completed the local cleanup and wrote
  `stale_active_release` audit evidence but stopped before the final tracker label
  mutation, the same recovery command may reenter only when the remaining blockers
  are stale protocol/activity summaries from the already-terminal run.
  If the final active-label mutation already completed while the queue label remains
  and the issue stayed in the configured in-progress state, the command may reenter
  as `stale_active_state_restore_pending` and restore only the first configured
  startable state after rechecking the same run/attempt audit and cleanup evidence.
- `Recovery Worktrees` means the path is retained local state after the authoritative
  runtime owner is gone or cannot explain it as active, review/landing, or queued
  work.
- `retained_attention` in `Recovery Worktrees` means the durable Run Ledger final
  outcome for the same issue is `needs_attention` or `terminal_failure`. This is a
  human-required retained lane, not neutral cleanup hygiene. The project summary
  `attention_count` includes it because the retained worktree is current recovery
  state even when `queued_candidates` is empty and no active or post-review lane
  currently owns the issue. A terminal Run Ledger attention row without a retained
  worktree, queued attention row, active or needs-attention tracker label, or blocked
  post-review lane is history-only and must not inflate current attention. When the
  same issue is currently owned by a non-attention `Review & Landing` row such as
  `wait_for_review` or `ready_to_land`, that row controls the current action summary;
  stale active-label or worktree echoes from an older terminal ledger record stay in
  Run Ledger history instead of reappearing as current attention.
- `decodex lane inspect` applies the same issue-scoped terminal Run Ledger projection
  to old or unowned runs. Cleanup-complete history renders as
  `status=cleanup_complete`, `ownership_state=closed`, `liveness_state=not_running`,
  and `lane_control_next_action=no_action`; terminal failure history renders as
  retained attention instead of stale `review_handoff_pending` or `running`. A still
  leased current attempt is not overwritten by older terminal ledger history.
- If private evidence shows `phase_goal_recovery` followed by a queued continuation,
  the lane is not a retained-attention worktree even when the preceding child failed
  or stalled reconciliation first found retained dirty progress. Treat it as
  Decodex-owned re-entry into the next phase unless a later terminal Run Ledger row,
  current attention signal, authority decision request, or blocker checkpoint
  supersedes it.

Every operator snapshot worktree row includes `ownership`, `ownership_reason`,
`provenance`, and optional `recovery_next_action` fields that distinguish current-lane
ownership, post-review ownership, queued attention, retained attention, post-land
cleanup, and cleanup-only local retention. Runtime-recorded mappings report
`provenance.source =
"runtime_recorded"` with created and refreshed Unix timestamps. Deterministically
rebuilt mappings report `provenance.source = "runtime_recovered"` when tracker,
retained lifecycle record, or closeout evidence proves a current owner after local state was
missing. Filesystem-only scans use scan-specific provenance such as `filesystem_scan`
or `git_hygiene_scan`. Rows migrated from older runtime stores that had no provenance
report `provenance.source = "legacy_unknown"` and may set
`provenance.audit_required = true`.

A runtime-recorded mapping with an identifier-style `issue_id`, no active lease or
shared claim, no retained review lifecycle or review checkpoint authority, a checkout
path that is confirmed missing, and a latest terminal run attempt is classified as
stale terminal local residue. Live status and post-review readback skip Linear refresh
and ledger calls for that row, omit it from `Recovery Worktrees`, and surface
`stale_terminal_local_worktree_mapping_ignored`; review-handoff recovery diagnose
surfaces `stale_terminal_local_residue` for the skipped local row without refreshing
the identifier through Linear. Project reconciliation clears the mapping before issue
selection so it cannot poison targeted dispatch. Filesystem uncertainty is not treated
as a missing checkout path; Decodex fails closed instead of clearing local runtime
authority.

A `Recovery Worktrees` row tells the operator to inspect the local path and either
clean it up or recover local-only changes; it is not, by itself, evidence that the
SQLite runtime store lost a current lane. When the tracker issue is already `Done`,
the row has runtime provenance, and no retained lane owns the worktree, the row is
neutral cleanup-only state, not a blocking recovery error.

When a retained worktree reports `role: cleanup_only`, treat it as local cleanup
hygiene rather than a current lane. It does not imply that an agent, child
process, post-review repair, closeout, or queued recovery run is still executing,
and it is not queue pressure or a hidden active issue claim. The row only says local
disk still has a retained checkout after the runtime owner is gone; once the
operator verifies the issue or PR is terminal, `main` contains the intended work,
and the checkout has no local-only changes that need recovery, the safe action is
to remove that local worktree.

If that same cleanup-only row reports `provenance.source = "legacy_unknown"` and
`audit_required = true`, treat it as a legacy orphan cleanup decision rather than
ordinary hygiene. Decodex is explicitly saying it cannot prove PR or closeout lineage
from durable runtime records. Do not use review-handoff rebind for this state unless a
separate diagnosis finds an open PR lane with exact lineage. Verify the tracker issue
and PR terminal state, inspect the checkout, run
`decodex recover legacy-closeout <ISSUE> --pr <MERGED_PR> --dry-run`, rerun with
`--manual-authority` only after validation passes, and then remove the local worktree
only after that evidence is understood.

If the row is not a real retained checkout and the issue is already completed because
a human merged the PR outside Decodex, use the formal stale-attention reconciliation
path instead of rerunning the lane: verify the PR URL, run
`decodex recover merged-closeout <ISSUE> --pr <MERGED_PR> --dry-run`, then rerun with
`--manual-authority` only after it proves the issue is completed, queue/active/attention
labels are absent, the PR head branch matches the retained branch, and the merge commit
is reachable from the current local `origin/<default-branch>`. Successful recovery
writes `closeout` plus `cleanup_complete` ledger records and should remove the false
project attention count.

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
- When a worker has recorded `issue_terminal_finalize` but the surrounding retained
  lifecycle has not finished writing back, the operator projection uses
  `phase = terminal_pending`. Statuses such as `review_handoff_pending`,
  `review_repair_pending`, `closeout_pending`, and `manual_attention_pending` mean
  Decodex is finishing the terminal path. They are not active execution, do not hold a
  queue lease, do not count as suspected stalls, and should not expose hard-interrupt
  fallback as an available control.
  Handoff and repair writeback gaps must use deterministic wait reasons such as
  `review_handoff_writeback_missing_lifecycle_marker`,
  `review_repair_writeback_missing_lifecycle_marker`, or
  `review_repair_writeback_stale_lifecycle_marker` instead of projecting an ordinary
  implementation lane as pending review work.
- Child-agent activity comes from `.decodex-run-activity` when the app-server recorder
  captured model/tool/tracker/browser/image buckets.
- The child-agent breakdown is diagnostic. It explains where observed wall time went;
  it is not a scheduler contract.
- Missing child-agent activity means no breakdown was captured for that run, not that
  the lane is invalid.
- Review-policy checkpoint state comes from runtime SQLite, not marker files. Legacy
  marker fields such as `review_policy_status` may explain an old worktree, but they
  must not override the store-backed checkpoint row for handoff or repair gating.
  Ordinary running implementation lanes with no current checkpoint must remain
  `policy_state = allowed` with `lane_control_next_action = continue_owned_attempt`;
  status may synthesize a pending review only for an in-progress review-writeback
  operation or from current runtime checkpoint evidence. Terminal writeback gaps use
  terminal lifecycle summaries and deterministic wait reasons instead.

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
- Fine-grained retry budgets, review-policy checkpoints, structured accepted and
  rejected independent-review findings, raw attempts, heartbeat, child buckets, token
  pressure, recovery details, and process logs stay local. Logs are diagnostic text;
  private execution events are structured runtime evidence.
- Completed lanes without Decodex Linear execution ledger records are reported as
  `missing` / `execution_ledger_missing`. Tracker terminal state, local attempt
  success, and non-ledger comments never satisfy the Run Ledger outcome contract.
- When a terminal Run Ledger attention record exists for an issue that still has both
  the service queue label and `decodex:needs-attention`, the operator snapshot treats
  the queue label as stale echo state. The issue remains visible through Run Ledger
  attention and any retained-worktree or tracker-label attention signal instead of
  appearing again as intake backlog.

## Current Non-Goals

These directions were discussed but are not part of the current implemented contract:

- Active-lane UI controls for steer, retry, task replacement, or lifecycle mutation.
- User-visible conflict-domain scheduling for `ui-preview`, `docs`, `tests`,
  `runtime`, or similar lane classes. Future conflict-domain scheduling belongs to the
  internal Execution Program contract, not to ordinary dashboard graph controls.
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

- Loop-runtime contract: [`../spec/loop-runtime.md`](../spec/loop-runtime.md)
- Runtime contract: [`../spec/runtime.md`](../spec/runtime.md)
- Lane-control capability contract: [`../spec/lane-control.md`](../spec/lane-control.md)
- Linear execution ledger schema: [`../spec/linear-execution-ledger.md`](../spec/linear-execution-ledger.md)
- Pilot procedure: [`../runbook/self-dogfood-pilot.md`](../runbook/self-dogfood-pilot.md)
- Workspace layout: [`./workspace-layout.md`](./workspace-layout.md)

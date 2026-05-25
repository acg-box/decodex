# Agent Evidence

Purpose: Define the local, agent-readable evidence files Decodex writes for fast
debugging and recovery handoff.

Status: normative

Read this when: You need to know where an agent should start when diagnosing a
local Decodex automation run, blocker, retained lane, or connector outage.

Not this document: The runtime state machine, human runbook steps, Linear execution
ledger schema, or dashboard layout.

Defines: Local evidence paths, schemas, authority boundaries, and write triggers.

## Authority

Agent evidence is a derived diagnostic surface. It must not become the runtime source
of truth for leases, retries, retained PR state, queue selection, project
registration, or landing authority.

The runtime SQLite database remains authoritative for Decodex-owned local state. The
operator snapshot remains the shared local projection for status, dashboard, and
diagnosis. Agent evidence is a stable file projection of that snapshot so a repair
agent can start from one compact index instead of reconstructing state from logs,
Markdown notes, worktree names, and ad hoc SQL.

Private execution events are structured rows in the runtime SQLite database scoped by
project, issue, run, and attempt. They are the local-only ledger for full execution
evidence that should not be mirrored to Linear. Agent evidence files may point agents
toward the current runtime context, but they do not replace the private execution
event store.

## Path Layout

Agent evidence lives under the local Decodex home:

```text
~/.codex/decodex/agent-evidence/<service-id>/
  handoff-index.json
  events.jsonl
  blockers/<issue-or-project-key>.json
  runs/<yyyy-mm>/<run-id>/capsule.json
```

These files are local-only operator state. They are not committed to target
repositories, mirrored to Linear, or used as external collaboration records.
They are also not process logs; logs stay diagnostic, while private execution events
stay structured runtime evidence in SQLite.

## Write Triggers

Decodex writes evidence through two entrypoints:

- `decodex diagnose` generates evidence for one resolved project config and prints a
  one-line summary by default.
- `decodex diagnose --json` generates the same files and prints the
  `decodex.agent_handoff_index/1` JSON body.
- `decodex serve` refreshes evidence after each successfully built per-project
  operator snapshot. If evidence writing fails during `serve`, Decodex logs a warning
  and keeps the control plane running; evidence files must not block scheduling.

The `diagnose` command may fall back to local runtime state when tracker credentials
or live observer refresh is unavailable. In that case, the handoff index includes a
typed warning such as `diagnose_tracker_observer_unavailable` or
`diagnose_live_observer_unavailable`.

## Handoff Index Schema

`handoff-index.json` uses schema `decodex.agent_handoff_index/1`.

Required fields:

- `schema`: exactly `decodex.agent_handoff_index/1`
- `project_id`: service id for the evidence directory
- `generated_at`: UTC RFC 3339 timestamp
- `source`: `diagnose_command` or `serve_tick`
- `evidence_root`, `handoff_index_path`, `blockers_dir`, `runs_dir`, `events_path`:
  absolute local paths
- `summary`: counts for projects, active runs, recent runs, history lanes, queued
  candidates, post-review lanes, recovery worktrees, blockers, run capsules,
  connector backoffs, and warnings
- `warnings`: typed operator snapshot or diagnose warning strings
- `connector_backoffs`: typed connector wait records from the operator snapshot
- `blockers`: compact blocker refs with reason codes, next action, and snapshot path
- `run_capsules`: compact run refs with capsule paths
- `recovery_worktrees`: retained local worktrees that need cleanup or recovery context
- `recovery_contracts`: commands or next actions an agent can use for supported
  recovery classes

Consumers must treat unknown additive fields as non-breaking.

## Blocker Snapshot Schema

`blockers/<issue-or-project-key>.json` uses schema `decodex.blocker_snapshot/1`.

Each blocker carries:

- `surface`: `running_lane`, `intake_queue`, `review_landing`,
  `recovery_worktree`, `operator_snapshot`, or `connector_backoff`
- `classification`: the operator snapshot classification
- `reason_code`: a stable machine reason such as `suspected_stall`,
  `missing_dispatch_briefing`, `missing_review_handoff_record`, or
  `tracker_rate_limited`
- `next_action`: a short agent-facing recovery hint
- `related_run_capsule_path`: the run capsule path when the blocker belongs to a
  known run

For `missing_review_handoff_record`, the recovery contract must point agents to
`decodex recover review-handoff diagnose <ISSUE> --json`. Rebind remains an explicit
validated recovery action and must not be inferred from branch names, current HEAD,
Linear comments, or the evidence file alone.

## Run Capsule Schema

`runs/<yyyy-mm>/<run-id>/capsule.json` uses schema `decodex.run_capsule/1`.

The capsule captures the compact runtime state an agent needs before opening a
worktree:

- issue id, issue identifier, title, run id, attempt number
- status, raw attempt status, phase, wait reason, current operation
- queue lease state and execution liveness
- thread, turn, process, protocol event, idle, and progress fields
- effective model/provider/cwd/approval/sandbox fields when known
- branch and worktree path
- optional Run Ledger outcome
- `diagnosis.attention_required`, `diagnosis.reason_code`, and
  `diagnosis.next_action`

Capsules are rewritten snapshots, not append-only event logs. The append-only stream
is `events.jsonl`.

## Event Stream

`events.jsonl` uses schema `decodex.agent_evidence_event/1`.

Each line records one evidence write with source, project id, handoff index path,
blocker count, run capsule count, warning count, and connector backoff count. This
stream exists so a future agent can identify when evidence changed without diffing
all JSON files.

The event stream is append-only between maintenance windows, but it is not permanent
runtime authority. `decodex maintenance prune --apply` and the auto-safe maintenance
subset in `decodex serve` may copy-truncate an oversized `events.jsonl` into a rotated
local sibling file and later delete old rotated event files. The current
`handoff-index.json`, blocker snapshots, and run capsules remain the compact diagnostic
surface for repair agents.

## Privacy Boundary

Agent evidence may include local filesystem paths, issue identifiers, PR URLs,
branch names, run ids, thread ids, model names, status classifications, and compact
next actions. It must not intentionally include raw model transcripts, full command
output, secret values, API tokens, or unredacted connector error bodies.

---
type: "Reference"
title: "Runtime Contracts"
openwiki_generated: true
---

# Runtime Contracts

Scope: current v0.2 behavior only. For vNext target implementation, the
[vNext authority contract](vnext-authority.md) supersedes this page's Linear lane,
SQLite, Goal, transport, lifecycle, and authority model.

This page is the compact contract map for Decodex runtime behavior. It consolidates the checked-in OpenWiki pages with the legacy runtime, app-server, tracker-tool, and workflow-file specs inspected from git history, while treating current Rust source as authority.

Use this page before changing lease ownership, app-server dispatch, tracker writeback, evidence/privacy boundaries, project config, `WORKFLOW.md`, or recovery behavior. For deeper lifecycle detail, continue to [Runtime lifecycle](runtime-lifecycle.md); for schema and row-family detail, continue to [Contracts and data](contracts-and-data.md).

## Runtime State Ownership

Decodex is local-first. The runtime home is `~/.codex/decodex`, with global config, `accounts.jsonl`, project contract directories, logs, agent evidence, and the single-machine SQLite database resolved by `apps/decodex/src/runtime/paths.rs`.

The runtime SQLite database at `~/.codex/decodex/runtime.sqlite3` owns active and retained execution truth. Schema bootstrap creates durable rows for projects, leases, run attempts, protocol events and summaries, run activity summaries, worktree mappings, and Linear execution-event mirrors, then installs additional schemas for review lifecycle, evidence artifacts, run-control channels, connector backoff, private execution events, Decision Contracts, autonomy state, Execution Programs, Program Intake, and loop guardrails (`apps/decodex/src/state/sqlite_store/schema.rs`).

Treat these as authority records, not caches:

- leases, run attempts, retry/backoff state, run-control channels, and worktree mappings
- protocol event identity and compact summaries used for startup/status readback
- private execution events, validation evidence, review checkpoints, and evidence artifacts
- retained PR lifecycle records and append-only lifecycle events
- Decision Contracts, Objective/autonomy records, Program Intake, and Execution Programs

Linear issues, Linear execution comments, GitHub PR metadata, logs, `.decodex-run-activity`, and agent-evidence files are public mirrors or diagnostics unless a runtime adapter has persisted them as structured runtime authority. Do not rebuild live ownership from Linear comments, PR titles, branch names, or raw logs.

## Project And `WORKFLOW.md` Contracts

A registered project is represented by a centralized directory under `~/.codex/decodex/projects/<service-id>/` containing `project.toml` and `WORKFLOW.md`. `ServiceConfig` resolves `project.toml`, repo root, worktree root, and the colocated workflow path; it does not discover workflow policy from the target repository root (`apps/decodex/src/config/service.rs`).

`project.toml` is strict TOML with denied unknown fields (`apps/decodex/src/config/document.rs`). It owns service identity, tracker credential environment-variable names, GitHub token and optional command routing, Codex review/account settings, autonomy references, privacy-classifier configuration, and repository/worktree paths. It may reference accepted runtime authority objects, but the config body does not itself replace accepted Objective Contract, Decision Contract, or project-policy records.

`WORKFLOW.md` is project-owned execution policy loaded through `WorkflowDocument` (`apps/decodex/src/workflow/document.rs`). It uses TOML frontmatter delimited by `+++`, denies unknown fields, and requires versioned tracker, agent, execution, workspace-hook, gate-profile, and context fields (`apps/decodex/src/workflow/frontmatter.rs`, `apps/decodex/src/workflow/execution.rs`, `apps/decodex/src/workflow/tracker.rs`). The workflow owns tracker state names, labels, read-first context, canonicalization and verification commands, named gate profiles, retry/turn budgets, and worktree hooks. Runtime code still owns lease acquisition, worktree mapping, app-server attempts, retry scheduling, review lifecycle, landing/closeout, and recovery classification.

## Leases, Attempts, And Run Control

A lane is one issue-scoped unit of retained work: issue identity, deterministic branch/worktree mapping, local lease, one or more run attempts, protocol summaries, private execution evidence, and, after PR handoff, review lifecycle records.

Normal execution is single-owner:

1. Acquire or reuse the local lease for exactly one issue.
2. Prepare or reuse the deterministic linked worktree.
3. Start one app-server attempt with a `run_id` and `attempt_number`.
4. Persist protocol/private evidence and run-control metadata for that attempt.
5. Resolve exactly one continuation, retry, manual-attention, review-handoff, retained-review, landing, closeout, cleanup, or terminal failure path.

Run attempts move through explicit statuses in the runtime store; app-server execution records `starting`, `running`, `succeeded`, or `failed` around the child session (`apps/decodex/src/agent/app_server/run.rs`). Queued retries and continuations are runtime claims until they fire, are cancelled, or become ineligible. A terminal-looking child-process exit does not override a persisted successful terminal writeback, lifecycle record, or explicit terminal-finalize event.

Lane-control and MCP control surfaces must use current project/issue/run/attempt/thread/turn authority. Liveness evidence can keep a lane visible and diagnosable, but it cannot recreate lease ownership after the lease, run-control channel, or terminal authority is gone.

## App-Server Protocol Contract

Decodex launches Codex with `codex app-server --listen stdio://` for lane execution. Current source starts the app-server client, initializes, runs bounded capability preflight, optionally performs command-exec health checks, starts or resumes a thread, records the thread session, then runs the turn loop (`apps/decodex/src/agent/app_server/run.rs`).

Compatibility is capability-gated, not a broad version promise:

- `decodex probe stdio://` must complete the dynamic-tool probe with `PROBE_OK` (`apps/decodex/src/agent/app_server/constants.rs`).
- Schema probing requires Decodex-owned methods and markers including initialize, config/model/skills/plugin/MCP preflight reads, thread start/resume/archive, phase-goal methods, turn start, command exec, dynamic tool calls, function/namespace tool shapes, and required notifications (`apps/decodex/src/agent/app_server/schema_probe/constants.rs`).
- Runtime preflight records config, model, model-provider, skills, plugin, and MCP capability evidence before dispatch proceeds (`apps/decodex/src/agent/app_server/preflight.rs`).
- Phase-goal methods are required for retained lane execution; missing support is a typed app-server compatibility blocker, not permission to silently continue without goals.

Protocol journals store ordered event identity and payload digests. `thread/archive` and `thread/archive/discarded` are terminal protocol barriers; later non-terminal events are discarded into post-archive diagnostic evidence and must not replace the terminal archive outcome (`apps/decodex/src/state/protocol_events/archive.rs`, `apps/decodex/src/agent/app_server/archive.rs`).

## Tracker Tools And Writeback

The child agent never receives broad tracker authority. `TrackerToolBridge` binds dynamic tool calls to the currently leased issue, current workflow, optional state store, review context, privacy classifier, PR inspector, and local repo inspector (`apps/decodex/src/agent/tracker_tool_bridge.rs`, `apps/decodex/src/agent/tracker_tool_bridge/construction.rs`).

Supported tool names are runtime-owned constants:

- `issue_transition`
- `issue_comment`
- `issue_label_add`
- `issue_progress_checkpoint`
- `issue_review_checkpoint`
- `issue_review_handoff`
- `issue_review_repair_complete`
- `issue_closeout_complete`
- `issue_terminal_finalize`

Writeback is private-first and disposition-driven. Progress checkpoints persist full normalized private payloads before any public Linear projection (`apps/decodex/src/agent/tracker_tool_bridge/tools/progress_checkpoint.rs`). Public Linear comments are rendered, allowlisted projections, not arbitrary agent-authored transcripts. Successful implementation requires explicit review handoff plus `issue_terminal_finalize(path = "review_handoff")`; manual attention requires a needs-attention label intent, validated public-safe explanatory summary, and `issue_terminal_finalize(path = "manual_attention")`. Missing, mixed, or unfinalized terminal signals must fail closed instead of being inferred from prose (`apps/decodex/src/agent/tracker_tool_bridge/tools/completion.rs`).

Review checkpoints are runtime-owned evidence writers for post-handoff or retained repair phases. They are not a generic escape hatch for child agents to redefine repair scope, review policy, or landing authority.

## Evidence And Privacy

Private/local surfaces include runtime SQLite private execution events, protocol summaries, review/evidence artifacts, Decision Contract and Program payloads, autonomy signals/proposals, run-control audit records, agent evidence under `~/.codex/decodex/agent-evidence`, logs under `~/.codex/decodex/logs`, account-pool files, and auth material.

Public/team surfaces include Linear issue state, labels, public lifecycle comments, GitHub PR metadata/status, public-safe dashboard/MCP projections, and generated normal issue briefs.

The privacy boundary is schema-first:

- Keep raw checkpoint focus, next action, blockers, evidence, local paths, account details, credentials, hidden reasoning, protocol payloads, internal graph ids, and private evidence ids out of Linear.
- Render only allowlisted public fields for tracker comments and status surfaces.
- If a local public-projection privacy classifier is configured, it receives already-selected public projection text fields only; it is a secondary guard, not the primary boundary (`apps/decodex/src/tracker/privacy_classifier.rs`).
- Suspicious or unavailable classifier results should fail closed by omitting optional public fields or using fixed public-safe fallback text, while preserving private runtime evidence.

## Recovery Boundaries

Recovery is evidence-bound and fail-closed. Startup and current-lane recovery may use runtime rows, deterministic worktree paths, tracker facts, PR facts, lifecycle records, run-control channels, protocol summaries, private execution events, review checkpoints, and `.decodex-run-activity` breadcrumbs only when they are scoped to the same project, issue, run id, and attempt. The recovery context loader opens the runtime store, resolves the project config, loads the centralized workflow, and only writes runtime registration when the selected recovery mode permits mutation (`apps/decodex/src/recovery/context/loader.rs`).

Do not recover ownership from these inputs alone:

- branch names, PR titles, or current `HEAD`
- Linear comments without matching runtime/lifecycle authority
- stale PID markers or logs
- retained diffs that cannot be tied to a lease, attempt, lifecycle record, or reviewed recovery path
- historical handoff/orchestration tables after lifecycle records became authoritative

Retained review lanes require exact lifecycle authority. Missing or mismatched lifecycle records block automatic post-review dispatch until an explicit diagnose/adopt/rebind path validates evidence and persists the reviewed projection. Dirty retained work after crash or stall must flow through retry, phase-goal recovery, repo-gate recovery, scoped authority recovery, or human-attention classification according to runtime evidence.

## Validation And Source References

When changing these contracts, validate both source and behavior:

- Frozen project/config parsing: `apps/decodex/src/config/service.rs` and
  `apps/decodex/src/config/document.rs`; use the trusted v0.2 tag for its historical
  example because the current `decodex.example.toml` now owns vNext global config.
- Workflow parsing and gate policy: `apps/decodex/src/workflow/document.rs`, `apps/decodex/src/workflow/frontmatter.rs`, `apps/decodex/src/workflow/execution.rs`, `apps/decodex/src/workflow/tracker.rs`, and workflow tests under `apps/decodex/src/workflow/tests/`.
- Runtime state and protocol persistence: `apps/decodex/src/runtime/paths.rs`, `apps/decodex/src/state/sqlite_store/schema.rs`, `apps/decodex/src/state/store.rs`, `apps/decodex/src/state/protocol_events/archive.rs`.
- App-server compatibility: `apps/decodex/src/agent/app_server/run.rs`, `apps/decodex/src/agent/app_server/preflight.rs`, `apps/decodex/src/agent/app_server/schema_probe/constants.rs`, `apps/decodex/src/agent/app_server/tests/`, and `decodex probe stdio://`.
- Tracker writeback and privacy: `apps/decodex/src/agent/tracker_tool_bridge.rs`, `apps/decodex/src/agent/tracker_tool_bridge/tools/`, `apps/decodex/src/agent/tracker_tool_bridge/tests/`, `apps/decodex/src/tracker/privacy_classifier.rs`.
- Recovery and lifecycle boundaries: recovery modules under `apps/decodex/src/recovery/`, review lifecycle modules under `apps/decodex/src/state/review_records/`, and post-review orchestration under `apps/decodex/src/orchestrator/`.

The legacy spec files inspected from git history were useful background, but current source, project contracts, tests, manifests, and runtime state remain authoritative. OpenWiki is the maintained explanatory knowledge surface; do not recreate `docs/`.

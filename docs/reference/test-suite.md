---
type: "Reference"
title: "Test Suite"
description: "Map the current test suite by behavior surface so pruning, additions, and debugging start from a shared inventory."
status: active
authority: current_state
owner: docs
tags: [reference]
code_refs: [apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs, apps/decodex/src/orchestrator/tests.rs, apps/decodex/src/agent/app_server/tests.rs, apps/decodex/src/state/tests.rs]
drift_watch: [cargo nextest list --workspace --all-targets --all-features, mcp::tests]
last_verified: 2026-06-19
---
# Test Suite

Purpose: Map the current test suite by behavior surface so pruning, additions, and
debugging start from a shared inventory.

Read this when: You need to decide where a Decodex test belongs, whether two tests can
be merged, or which dense test matrix is intentionally retained.

Not this document: Runtime truth, operator procedure, or the command authority for
repository checks.

Covers: Current test inventory, grouping rules, high-density test surfaces, and the
standards for keeping, merging, or deleting tests.

## Current Snapshot

This snapshot groups 1349 runnable `nextest` tests across all targets. One additional
skipped live app-server test is listed only with verbose or JSON inventory output.
Regenerate the runnable inventory with:

```sh
cargo nextest list --workspace --all-targets --all-features --verbose \
  | rg -v "\\(skipped\\)"
```

Regenerate the ignored/live-test inventory with:

```sh
cargo nextest list --workspace --all-targets --all-features --verbose \
  | rg "\\(skipped\\)|ignored"
```

Regenerate the top-level grouping with:

```sh
cargo nextest list --workspace --all-targets --all-features 2>/dev/null \
  | awk '{print $2}' \
  | sed 's/::[^:]*$//' \
  | sort \
  | uniq -c \
  | sort -nr
```

## Primary Groups

| Group | Count | Primary surfaces | Owns |
| --- | ---: | --- | --- |
| Orchestrator | 641 | `apps/decodex/src/orchestrator/tests.rs`, `apps/decodex/src/orchestrator/tests/**/*.rs` | Intake, retry, review/landing, runtime cleanup, operator status, repo gates |
| Tracker tool bridge | 101 | `apps/decodex/src/agent/tracker_tool_bridge/tests.rs`, `apps/decodex/src/agent/tracker_tool_bridge/tests/**/*.rs` | Dynamic tracker tools, continuation guards, review handoff writes, closeout writes |
| App-server protocol/runtime | 96 | `apps/decodex/src/agent/app_server/tests.rs`, `apps/decodex/src/agent/json_rpc.rs`, app-server protocol tests | JSON-RPC parsing, turn execution, dynamic tools, thread config, transport failures |
| Runtime state, locks, and maintenance | 93 | `state::tests`, `runtime::tests`, `maintenance::tests` | Persistent local state, lock ownership, runtime database contracts, local retention |
| Workflow, config, and docs parsing | 62 | `workflow::tests`, `config::tests`, `codex_config::tests`, `docs_okf::tests` | `WORKFLOW.md`, project config, Codex config edits, removed-field rejection, default policy, OKF docs parsing |
| Git, worktree, landing, and recovery helpers | 161 | `worktree::tests`, `manual::tests`, `commit_message::tests`, `github::tests`, `default_branch_sync::tests`, `pull_request::tests`, `recovery::tests`, `git_credentials::tests` | Git/worktree behavior, manual landing, GitHub/PR helpers, recovery commands, commit-message policy |
| Account, CLI, archive, tracker, and MCP integration | 155 | `accounts::tests`, `agent::codex_accounts::tests`, `agent::decodex_tool_bridge::tests`, `app_bridge::tests`, `cli::tests`, `archive_hygiene::tests`, `tracker::*::tests`, `mcp::tests`, `apps/decodex/tests/mcp_stdio.rs` | User-facing commands, account pools, app bridge parsing, archive hygiene, direct tracker adapter, MCP resources, HTTP transport, bearer auth, schema-bound planning tools, inspect-first operate/admin tools, remote-safe observability templates, process-level MCP smoke, and public-text behavior |
| Program intake | 10 | `program_intake::tests` | Decision Contract materialization, Linear issue shaping, and internal Execution Program persistence |
| Loop contract and research surfaces | 30 | `loop_contract::tests`, `execution_program::tests`, `research_design::tests`, `plugin_surface_tests` | Decision Contract schema, Execution Program readiness, research compile/promote, and plugin trigger behavior |

## Orchestrator Inventory

The orchestrator suite is intentionally split by lifecycle stage. Do not add another
large catch-all test file unless the behavior crosses several of these stages.

| File | Count | Group |
| --- | ---: | --- |
| `apps/decodex/src/orchestrator/tests/intake/workflow_reload.rs` | 4 | Workflow reload and cached policy snapshots |
| `apps/decodex/src/orchestrator/tests/intake/eligibility.rs` | 7 | Intake eligibility and queue label safety |
| `apps/decodex/src/orchestrator/tests/intake/run_and_prompting.rs` | 33 | Prompt construction, machine-only redaction, run setup |
| `apps/decodex/src/orchestrator/tests/intake/prepare_issue_run.rs` | 10 | Worktree preparation and pre-run guards |
| `apps/decodex/src/orchestrator/tests/intake/candidate_selection.rs` | 22 | Candidate ordering, retained lane preference, closeout dispatch policy |
| `apps/decodex/src/orchestrator/tests/retry/scheduling.rs` | 24 | Retry timing, dry-run behavior, retry marker semantics |
| `apps/decodex/src/orchestrator/tests/retry/selection.rs` | 14 | Retry queue selection and blocked retry candidates |
| `apps/decodex/src/orchestrator/tests/runtime/repo_gate.rs` | 9 | Repo gate command selection, cleanliness, shell fallback, and failure classification |
| `apps/decodex/src/orchestrator/tests/runtime_failure/` | 66 | Failure comments, retry markers, loop guardrails, handoff recovery, app-server failures, runtime credentials, cleanup, lease release |
| `apps/decodex/src/orchestrator/tests/runtime/loop_scenarios.rs` | 2 | Research-to-execution loop scenarios, Program dispatch readiness, phase-goal validation, review, guardrails, and harness feedback |
| `apps/decodex/src/orchestrator/tests/runtime/thread_archive.rs` | 1 | Completed-thread archive candidate filtering |
| `apps/decodex/src/orchestrator/tests/recovery/reconciliation.rs` | 24 | Stale lease, recovery worktree, and reconciliation behavior |
| `apps/decodex/src/orchestrator/tests/recovery/terminal_support.rs` | 0 | Shared retained recovery and closeout fixtures |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/dispatch.rs` | 5 | Direct closeout dispatch and PR validation |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/identity.rs` | 4 | Closeout identity reuse after retained runs |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/cleanup.rs` | 6 | Retained closeout cleanup and cleanup blockers |
| `apps/decodex/src/orchestrator/tests/recovery/terminal_failures.rs` | 14 | Terminal failure labeling, nonretryable attention, and local/remote duplicate writeback idempotency |
| `apps/decodex/src/orchestrator/tests/recovery/runtime_reentry.rs` | 30 | Runtime reentry, recovered worktrees, liveness, and live-run recovery |
| `apps/decodex/src/orchestrator/tests/operator/status_support.rs` | 0 | Shared operator status fixtures |
| `apps/decodex/src/orchestrator/tests/operator/status/control_plane.rs` | 10 | Registered project control-plane rows |
| `apps/decodex/src/orchestrator/tests/operator/status/running_lanes.rs` | 37 | Running lanes, stalled lanes, current-lane hydration, and local worktrees |
| `apps/decodex/src/orchestrator/tests/operator/status/history.rs` | 7 | Run ledger and Linear history hydration |
| `apps/decodex/src/orchestrator/tests/operator/status/text.rs` | 10 | Human-readable operator status text |
| `apps/decodex/src/orchestrator/tests/operator/status/publishing.rs` | 11 | Snapshot publishing, degraded observers, and tracker backoff |
| `apps/decodex/src/orchestrator/tests/operator/status/queue.rs` | 18 | Intake queue classifications and shared-claim visibility |
| `apps/decodex/src/orchestrator/tests/operator/status/http.rs` | 31 | Operator dashboard HTTP pages/assets, `/livez`, WebSocket control, and removed snapshot-route responses |
| `apps/decodex/src/orchestrator/tests/operator/status/dashboard.rs` | 34 | Dashboard client rendering contracts |
| `apps/decodex/src/orchestrator/tests/operator/status/agent_evidence.rs` | 6 | Agent evidence snapshots and private evidence readback |
| `apps/decodex/src/orchestrator/tests/review_landing/status_support.rs` | 0 | Shared Review & Landing status fixtures |
| `apps/decodex/src/orchestrator/tests/review_landing/status_rows.rs` | 18 | Review & Landing status rows and handoff lineage |
| `apps/decodex/src/orchestrator/tests/review_landing/orchestration.rs` | 18 | Review orchestration, admin merge, and repair routing |
| `apps/decodex/src/orchestrator/tests/review_landing/status_markers.rs` | 1 | Review lifecycle readback handling and recovered targeted visibility |
| `apps/decodex/src/orchestrator/tests/review_landing/classification_review.rs` | 12 | Review repair, request-pending, stale handoff, merged PR classification |
| `apps/decodex/src/orchestrator/tests/review_landing/classification_checks.rs` | 16 | Required checks, GitHub token gates, GraphQL pagination/query shape |
| `apps/decodex/src/orchestrator/tests/review_landing/review_state.rs` | 2 | Pull-request review-state conversion from GitHub GraphQL nodes |

## Tracker Bridge Inventory

| File | Count | Group |
| --- | ---: | --- |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/dispatch.rs` | 24 | Tool argument validation, state transitions, label mutations, closeout dispatch |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/continuation.rs` | 13 | Continuation-blocking writes and reactivation safety |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/progress.rs` | 10 | Progress checkpoint comments and worktree path handling |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/review/policy.rs` | 23 | Internal-review stop policy, repair/writeback behavior, checkpoint handling |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/review/handoff.rs` | 21 | Review handoff, repair complete, terminal finalize, closeout complete |

## Keep Standards

Keep separate tests when the case protects a different observable contract:

- Different public surface, such as CLI output, operator status JSON, tracker comments,
  Git commands, runtime database state, or app-server protocol payloads.
- Different state-machine outcome, especially blocked versus ineligible, retryable versus
  terminal, repair versus closeout, or queued versus retained.
- Different persisted lifecycle semantics, such as review handoff lineage, retry
  schedule state, cleanup handoff state, or closeout identity reuse.
- Different authority boundary, such as GitHub token routing, Linear tracker writes,
  repo-local Git config, or runtime-only state.
- Different process or concurrency boundary, such as active child reconciliation, lock
  contention, stale lease cleanup, or app-server transport failure.

## Merge Standards

Prefer table-driven tests when all cases exercise the same branch and assert the same
observable contract:

- Same tool rejects several invalid argument spellings.
- Same policy accepts or rejects multiple equivalent labels, transitions, or field names.
- Same redaction or prompt rule varies only fixture text.
- Same pagination guard varies only which stable metadata field changed.
- Same missing-configuration rule varies only absent versus blank environment values.

The merged test name should describe the behavior contract, not the fixture shape.

## Current Cleanup Targets

The first low-risk cleanup already collapsed equivalent CLI parser sibling tests plus
same-field config/workflow negative cases into table-driven coverage. Continue pruning
in this order:

- Remaining `cli::tests`, `config::tests`, and `workflow::tests` when cases only vary
  argument spelling, missing-field name, or invalid value text.
- `operator/status/dashboard.rs` when assertions inspect raw CSS or JavaScript text
  rather than a stable operator-facing contract.
- Large orchestrator lifecycle files only after proving the candidate cases share the
  same entrypoint, branch, state setup, persisted marker semantics, and externally
  visible assertion.

Keep the ignored
`agent::app_server::tests::live_app_server_resume_round_trip_updates_marker_and_state`
test out of the default gate unless the local app-server binary becomes deterministic
enough for normal CI.

## Delete Standards

Delete a test only when another remaining test is a strict behavioral superset:

- Same entrypoint.
- Same state setup except irrelevant spelling.
- Same branch or failure class.
- Same externally visible assertion.

Do not delete a test only because its fixture looks similar. Similar fixtures often
protect different contracts in retained review lanes, status rows, and cleanup flows.

## Intentionally Dense Areas

These areas should stay dense unless the implementation contract changes:

- `operator/status/` covers operator-facing JSON, text, dashboard, `/livez`, and
  WebSocket behavior. These tests are noisy but protect the local control-plane surface.
- `review_landing/status_rows.rs` keeps both descendant handoff and lineage-rewrite cases
  because the production branch distinguishes accepted ancestry from rejected rewrites.
- `intake/candidate_selection.rs` keeps planner, dispatch policy, and block-reason cases
  separate when they expose different operator or API outcomes.
- `retry/scheduling.rs` keeps retry and review-repair failure modes separate when the
  lane identity changes the marker or operator-facing reason.
- `recovery/closeout/` and `recovery/runtime_reentry.rs` keep closeout identity,
  cleanup handoff, and retained worktree
  recovery matrices separate because they preserve different recovery authority.

## Placement Rules

- Add orchestrator tests to the lifecycle subdirectory that owns the behavior surface
  above.
- Add tracker bridge tests to `mutation/` or `review/`, not the shared harness.
- Add a new file only when a behavior family has several tests and no existing group owns
  its lifecycle stage.
- Convert same-contract variants into table rows before adding sibling tests.
- Update this document when a new test family is created or when a dense matrix is
  intentionally collapsed.

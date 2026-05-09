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

This cleanup keeps 685 `nextest` tests plus one ignored live app-server test. Regenerate
the runnable inventory with:

```sh
cargo nextest list --workspace --all-targets --all-features
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
| Orchestrator | 359 | `apps/decodex/src/orchestrator/tests.rs`, `apps/decodex/src/orchestrator/tests/**/*.rs` | Intake, retry, review/landing, runtime cleanup, operator status, repo gates |
| Tracker tool bridge | 85 | `apps/decodex/src/agent/tracker_tool_bridge/tests.rs`, `apps/decodex/src/agent/tracker_tool_bridge/tests/**/*.rs` | Dynamic tracker tools, continuation guards, review handoff writes, closeout writes |
| App-server protocol/runtime | 38 | `apps/decodex/src/agent/app_server/tests.rs`, `apps/decodex/src/agent/json_rpc.rs`, app-server protocol tests | JSON-RPC parsing, turn execution, dynamic tools, thread config, transport failures |
| Runtime state and locks | 39 | `state::tests`, `runtime::tests` | Persistent local state, lock ownership, runtime database contracts |
| Workflow and config parsing | 53 | `workflow::tests`, `config::tests` | `WORKFLOW.md`, project config, removed-field rejection, default policy |
| Git, worktree, and landing helpers | 93 | `worktree::tests`, `manual::tests`, `commit_message::tests`, `github::tests`, `default_branch_sync::tests`, `pull_request::tests` | Git/worktree behavior, manual landing, GitHub/PR helpers, commit-message policy |
| CLI, archive, and tracker integration | 18 | `cli::tests`, `archive_hygiene::tests`, `tracker::linear::tests` | User-facing commands, archive hygiene, direct Linear adapter behavior |

## Orchestrator Inventory

The orchestrator suite is intentionally split by lifecycle stage. Do not add another
large catch-all test file unless the behavior crosses several of these stages.

| File | Count | Group |
| --- | ---: | --- |
| `apps/decodex/src/orchestrator/tests/intake/workflow_reload.rs` | 4 | Workflow reload and cached policy snapshots |
| `apps/decodex/src/orchestrator/tests/intake/eligibility.rs` | 7 | Intake eligibility and queue label safety |
| `apps/decodex/src/orchestrator/tests/intake/run_and_prompting.rs` | 38 | Prompt construction, machine-only redaction, run setup |
| `apps/decodex/src/orchestrator/tests/intake/prepare_issue_run.rs` | 10 | Worktree preparation and pre-run guards |
| `apps/decodex/src/orchestrator/tests/intake/candidate_selection.rs` | 24 | Candidate ordering, retained lane preference, closeout dispatch policy |
| `apps/decodex/src/orchestrator/tests/retry/scheduling.rs` | 28 | Retry timing, dry-run behavior, retry marker semantics |
| `apps/decodex/src/orchestrator/tests/retry/selection.rs` | 16 | Retry queue selection and blocked retry candidates |
| `apps/decodex/src/orchestrator/tests/runtime/repo_gate.rs` | 8 | Repo gate command selection, cleanliness, shell fallback, and failure classification |
| `apps/decodex/src/orchestrator/tests/runtime/failure.rs` | 35 | Failure comments, runtime credentials, cleanup, lease release |
| `apps/decodex/src/orchestrator/tests/recovery/reconciliation.rs` | 18 | Stale lease, recovery worktree, and reconciliation behavior |
| `apps/decodex/src/orchestrator/tests/recovery/terminal_support.rs` | 0 | Shared retained recovery and closeout fixtures |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/dispatch.rs` | 4 | Direct closeout dispatch and PR validation |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/identity.rs` | 6 | Closeout identity reuse after retained runs |
| `apps/decodex/src/orchestrator/tests/recovery/closeout/cleanup.rs` | 6 | Retained closeout cleanup and cleanup blockers |
| `apps/decodex/src/orchestrator/tests/recovery/terminal_failures.rs` | 8 | Terminal failure labeling and nonretryable attention |
| `apps/decodex/src/orchestrator/tests/recovery/runtime_reentry.rs` | 25 | Runtime reentry, recovered worktrees, liveness, and live-run recovery |
| `apps/decodex/src/orchestrator/tests/operator/status_support.rs` | 0 | Shared operator status fixtures |
| `apps/decodex/src/orchestrator/tests/operator/status/control_plane.rs` | 3 | Registered project control-plane rows |
| `apps/decodex/src/orchestrator/tests/operator/status/running_lanes.rs` | 22 | Running lanes, stalled lanes, active-run hydration, and local worktrees |
| `apps/decodex/src/orchestrator/tests/operator/status/history.rs` | 4 | Run ledger and Linear history hydration |
| `apps/decodex/src/orchestrator/tests/operator/status/text.rs` | 4 | Human-readable operator status text |
| `apps/decodex/src/orchestrator/tests/operator/status/publishing.rs` | 6 | Snapshot publishing, degraded observers, and tracker backoff |
| `apps/decodex/src/orchestrator/tests/operator/status/queue.rs` | 8 | Intake queue classifications and shared-claim visibility |
| `apps/decodex/src/orchestrator/tests/operator/status/http.rs` | 10 | Operator `/state`, `/livez`, readiness, and dashboard route responses |
| `apps/decodex/src/orchestrator/tests/operator/status/dashboard.rs` | 3 | Dashboard client rendering contracts |
| `apps/decodex/src/orchestrator/tests/review_landing/status_support.rs` | 0 | Shared Review & Landing status fixtures |
| `apps/decodex/src/orchestrator/tests/review_landing/status_rows.rs` | 18 | Review & Landing status rows and handoff lineage |
| `apps/decodex/src/orchestrator/tests/review_landing/orchestration.rs` | 12 | Review orchestration, admin merge, and repair routing |
| `apps/decodex/src/orchestrator/tests/review_landing/status_markers.rs` | 2 | Review orchestration marker handling and recovered targeted visibility |
| `apps/decodex/src/orchestrator/tests/review_landing/classification_review.rs` | 13 | Review repair, request-pending, stale handoff, merged PR classification |
| `apps/decodex/src/orchestrator/tests/review_landing/classification_checks.rs` | 15 | Required checks, GitHub token gates, GraphQL pagination/query shape |
| `apps/decodex/src/orchestrator/tests/review_landing/review_state.rs` | 2 | Pull-request review-state conversion from GitHub GraphQL nodes |

## Tracker Bridge Inventory

| File | Count | Group |
| --- | ---: | --- |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/dispatch.rs` | 22 | Tool argument validation, state transitions, label mutations, closeout dispatch |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/continuation.rs` | 13 | Continuation-blocking writes and reactivation safety |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/mutation/progress.rs` | 5 | Progress checkpoint comments and worktree path handling |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/review/policy.rs` | 22 | Internal-review stop policy, repair/writeback behavior, checkpoint handling |
| `apps/decodex/src/agent/tracker_tool_bridge/tests/review/handoff.rs` | 23 | Review handoff, repair complete, terminal finalize, closeout complete |

## Keep Standards

Keep separate tests when the case protects a different observable contract:

- Different public surface, such as CLI output, operator status JSON, tracker comments,
  Git commands, runtime database state, or app-server protocol payloads.
- Different state-machine outcome, especially blocked versus ineligible, retryable versus
  terminal, repair versus closeout, or queued versus retained.
- Different persisted marker semantics, such as review handoff lineage, retry schedule
  marker, cleanup handoff marker, or closeout identity reuse.
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
  readiness behavior. These tests are noisy but protect the local control-plane surface.
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

# Contracts And Data

This page is the primary map for Decodex contracts and data boundaries. Use it to identify the source file or test area that owns behavior before editing.

## Project contract

A registered project directory contains `project.toml` and `WORKFLOW.md` (`apps/decodex/src/config/service.rs`). The project config parser denies unknown fields (`apps/decodex/src/config/document.rs`). The safe setup model is `decodex.example.toml`.

Current `project.toml` shape:

```toml
service_id = "your-service-id"

[tracker]
api_key_env_var = "LINEAR_API_KEY"

[github]
token_env_var = "GITHUB_TOKEN"

[codex]
review = "strict"

[paths]
repo_root = "/absolute/path/to/target/repo"
worktree_root = ".worktrees"
```

Optional blocks include `[autonomy]`, `[autonomy.runtime_policy]`, `[codex.accounts]`, and `[privacy_classifier]` (`decodex.example.toml`). Config stores names of environment variables and references to runtime authority records; it does not embed live credentials or replace accepted Objective Contract/project-policy records.

## Workflow policy

`WORKFLOW.md` is project-owned execution policy. The runtime loads it through `WorkflowDocument` before each configured cycle (`apps/decodex/src/orchestrator/run_cycle.rs`). Source and old specs show these concerns are workflow-owned:

- tracker states and labels
- read-first files and prompting policy
- repo gate and validation commands
- canonicalization commands used by the baseline guard
- workspace hooks at worktree create/remove boundaries

Workflow policy constrains runtime behavior; it does not own runtime lifecycle authority. Worktree creation, leases, retries, post-review lifecycle, landing, closeout, cleanup, and recovery remain runtime-owned.

## Runtime database

The local SQLite database at `~/.codex/decodex/runtime.sqlite3` is the single-machine source of truth (`apps/decodex/src/runtime/paths.rs`, `openwiki/specs/contracts-and-data.md`). Schema bootstrap (`apps/decodex/src/state/sqlite_store/schema.rs`) creates:

- `projects`: registered project configs and enabled state.
- `leases`: one active local issue lease per issue.
- `run_attempts`: run id, project, issue, attempt, status, thread/turn ids, timestamps.
- `protocol_events`: keyed by `(run_id, sequence_number)` with payload SHA-256, not raw payload body.
- `protocol_event_summaries`: compact startup/status readback.
- `run_activity_summaries`: child-agent/protocol activity summary JSON.
- `worktrees`: issue-to-branch/worktree mappings.
- `linear_execution_events`: public tracker ledger event rows.

Bootstrap then installs schemas for worktrees, review lifecycle, evidence artifacts, run-control channels, connector backoff, private execution events, Decision Contracts, autonomy objectives/signals/proposals, Execution Programs, Program Intake state, and loop guardrails.

Do not rebuild runtime truth from Linear comments on each tick. Linear comments are a public mirror; local SQLite owns active execution and retained lifecycle truth.

## Leases, attempts, and run control

A Decodex lane is one issue, one branch/worktree mapping, and one or more run attempts. The runtime owns:

- issue lease acquisition/release
- dispatch slot and shared-claim checks
- attempt status transitions
- app-server child ownership
- run-control channel publish/retire
- retry budget and due-time scheduling
- recovery classification after crashes or stale children

Lane-control commands and MCP tools must use current run/turn authority instead of injecting commands into arbitrary threads (`apps/decodex/src/cli/control_commands/lane.rs`, `apps/decodex/src/mcp.rs`).

## App-server protocol contract

`apps/decodex/src/agent/app_server/run.rs` is the source entrypoint for one attempt. The runtime contract is:

- Decodex starts `codex app-server --listen stdio://`.
- Generated app-server JSON schema is more authoritative than stale handwritten assumptions.
- `decodex probe stdio://` should pass with `PROBE_OK` for compatibility evidence.
- Required runtime methods include initialize, thread start/resume, turn start, thread archive, command exec health checks, dynamic tool calls, and phase-goal methods.
- Phase goals are mandatory for retained lane execution; Decodex rejects incompatible app-server builds instead of silently falling back to ordinary continuation.

When changing app-server behavior, inspect source, schema tests, and upstream Codex behavior when available. Do not tune liveness or terminal semantics from one local pilot run only.

## Tracker tool contract

The tracker bridge binds agent writes to the currently leased issue (`apps/decodex/src/agent/tracker_tool_bridge.rs`). Tool constants include:

- `issue_transition`
- `issue_comment`
- `issue_label_add`
- `issue_progress_checkpoint`
- `issue_review_checkpoint`
- `issue_review_handoff`
- `issue_review_repair_complete`
- `issue_closeout_complete`
- `issue_terminal_finalize`

Important boundaries from source and `openwiki/specs/contracts-and-data.md`:

- The coding agent may write only to the leased issue through supported operations.
- Generic arbitrary comments are not allowed; structured public summaries are rendered by Decodex.
- Private checkpoint payloads go to runtime SQLite before public Linear projection.
- Public projection text must omit raw local evidence, credentials, paths, account details, and hidden reasoning.
- Completion must produce exactly one valid terminal path and matching `issue_terminal_finalize`; Decodex must fail rather than infer intent.
- Review checkpoints are runtime-owned evidence writers for post-handoff/repair phases, not a generic agent escape hatch.

## Decision Contracts, Program Intake, and Execution Programs

The loop runtime sits above individual issue lanes (`openwiki/specs/contracts-and-data.md`, `apps/decodex/src/loop_contract.rs`, `apps/decodex/src/program_intake.rs`, `apps/decodex/src/execution_program.rs`).

Decision Contract statuses:

- `draft_latent`: candidate only, not executable.
- `accepted_promoted`: explicit accepted authority; may feed Program Intake.
- `needs_human_decision`: blocked until a human decides.
- `rejected_superseded`: must not become executable work.

Program Intake kinds:

- `goal_intake`: materialize an accepted Decision Contract.
- `issue_batch_intake`: materialize supplied existing Linear issues.

Execution Programs are private runtime plans over normal issue-backed nodes. Scheduler dispatches ready nodes directly with `program` dispatch mode; queue labels are not Program scheduling. Public issue briefs may describe objectives, dependencies, validation, risks, and acceptance criteria, but must not expose internal graph ids, node ids, proposal ids, private evidence paths, or runtime row details.

## Review lifecycle records

After PR-backed handoff, retained review/landing/closeout lifecycle authority is the runtime DB `review_lifecycle_records` projection plus append-only private lifecycle events (`openwiki/specs/contracts-and-data.md`). Source areas include:

- `apps/decodex/src/orchestrator/kernel/post_review.rs`
- `apps/decodex/src/orchestrator/kernel/lifecycle.rs`
- `apps/decodex/src/orchestrator/retained_review_orchestration.rs`
- `apps/decodex/src/state/review_records/lifecycle.rs`

The pure lifecycle kernel decides from normalized facts; adapters perform side effects and persist projections. Linear comments, manual receipts, local branch names, PR titles, and current head heuristics are not final lifecycle authority.

Fail-closed rule: if a retained lane lacks an exact lifecycle authority projection, normal/program/retry dispatch must not guess a lineage. Use explicit `decodex recover review-handoff diagnose`, `adopt`, or `rebind` paths depending on evidence.

## Commit message contract

`decodex commit` and `decodex land` use `decodex/commit/2` for tree-change subjects (`apps/decodex/src/cli/manual_commands.rs`, `openwiki/specs/contracts-and-data.md`). Canonical shape:

```json
{"schema":"decodex/commit/2","change":"short semantic summary","authority":"XY-123","impact":"compatible"}
```

Allowed authority values are a Linear issue identifier, `baseline`, or `manual`. Allowed impact values are `compatible` and `breaking`. Do not put PR URLs, branches, validation status, closeout state, cleanup state, CI status, retry state, or related issue lists into the commit subject. Those belong to lifecycle records, PR/tracker metadata, or local validation evidence.

## Privacy and public/private split

Public surfaces:

- Linear issue state, labels, and public lifecycle comments.
- GitHub PR status, review state, and commit statuses.
- Dashboard/MCP public-safe operator projections.
- Generated normal issue briefs.

Private/local surfaces:

- Runtime SQLite private execution events.
- Decision Contract and Program payloads.
- Autonomy signals/proposals.
- Run-control audit records.
- Protocol event details and summaries.
- Agent evidence under `~/.codex/decodex/agent-evidence`.
- Logs under `~/.codex/decodex/logs`.
- Account pool files and auth material.

If adding a field to a public projection, first identify whether it can expose local paths, credentials, raw logs, hidden reasoning, account identity, private evidence ids, or internal graph mechanics. If yes, keep it private or add a sanitized summary only.

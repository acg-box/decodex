# Runtime Architecture

This page explains how the Decodex runtime is wired so future agents can change it without rediscovering every module. It focuses on current source behavior and uses OpenWiki as a navigation layer over checked-in authority.

## Workspace shape

The Rust workspace includes `apps/*` except `apps/decodex-app` (`Cargo.toml`). The main runtime package is `apps/decodex`, with dependencies on `clap`, `rusqlite`, `reqwest`, `serde`, `toml`, tracing, and hash/time utilities (`apps/decodex/Cargo.toml`). Radar and Publisher are independent Rust CLIs in the same workspace (`apps/radar/Cargo.toml`, `apps/decodex-publisher/Cargo.toml`).

The `decodex` crate exposes only a few public modules (`app_bridge`, `config`, `state`, `workflow`) and keeps most runtime machinery private (`apps/decodex/src/lib.rs`). That is intentional: the binary is an operator/runtime control plane, not a general library API.

## CLI bootstrap

`apps/decodex/src/main.rs` calls `decodex::run()`. `run()` does four things (`apps/decodex/src/lib.rs`):

1. Install `color_eyre` error reporting.
2. Initialize daily rolling tracing files in `~/.codex/decodex/logs`.
3. Install a panic hook that aborts after the default panic output.
4. Parse and run the Clap CLI.

The crate has a compile-time Unix-only guard: macOS and Linux are supported; Windows is rejected (`apps/decodex/src/lib.rs`).

`apps/decodex/src/cli.rs` owns the command surface. Current top-level commands include:

- `run`, `serve`, `status`, `project`, `lane`, `diagnose`, `evidence`, `recover`, `intake`, `mcp`, `probe`, `verify`
- `commit`, `land`, `git-hook` for Decodex-owned Git lifecycle policy
- `account`, `app`, `archive-linear`, `maintenance`
- hidden `_attempt` for daemon-planned child attempts

## Runtime state layout

All local runtime state is under `~/.codex/decodex` (`apps/decodex/src/runtime/paths.rs`):

- `config.toml`: global operator config.
- `accounts.jsonl`: shared ChatGPT/Codex account pool.
- `projects/`: registered project contract directories.
- `logs/`: Decodex tracing logs.
- `agent-evidence/`: derived repair-agent evidence views.
- `runtime.sqlite3`: single-machine runtime database.

`StateStore` opens the runtime DB and bootstraps schema with WAL enabled (`apps/decodex/src/state/sqlite_store/schema.rs`). Base tables include projects, leases, run attempts, protocol events and summaries, run activity summaries, worktrees, and Linear execution events. Bootstrap then adds worktree, review, evidence artifact, run-control, connector backoff, private execution event, Decision Contract, autonomy, Execution Program, Program Intake, loop guardrail, and migration schemas.

## Project contracts

A project is not discovered from a checkout. It is explicitly registered from a project directory containing `project.toml` and `WORKFLOW.md` (`apps/decodex/src/config/service.rs`, `apps/decodex/src/cli/control_commands/project.rs`). `project.toml` fields are parsed by `ServiceConfigDocument` (`apps/decodex/src/config/document.rs`):

- `service_id`
- `[tracker]`
- `[github]`
- optional `[codex]`
- optional `[autonomy]`
- optional `[privacy_classifier]`
- `[paths]`

`decodex.example.toml` is the safe redacted model. It stores env-var names such as `LINEAR_API_KEY` and `GITHUB_TOKEN`, not token values.

## One-shot run flow

`decodex run` is implemented by `apps/decodex/src/cli/control_commands/run.rs` and `apps/decodex/src/orchestrator/entrypoints/run.rs`.

The high-level flow is:

1. Open the global runtime store.
2. Resolve a project config from `--config`, current checkout registry mapping, or the registered project table.
3. Register or refresh the project config in runtime state.
4. Load the project `WORKFLOW.md`.
5. Respect stored tracker connector backoff.
6. Optionally explain the queue for `--dry-run --explain`.
7. Call `run_configured_cycle`.

`run_configured_cycle` loads `ServiceConfig`, workflow, and a Linear client. If an issue id is supplied, it runs that target issue with inferred or explicit dispatch mode; otherwise it runs project selection (`apps/decodex/src/orchestrator/run_cycle.rs`). Preparation validates workflow read-first files, plans worktree state, resolves run identity, acquires leases, and materializes the lane through the run-cycle modules.

Recent source adds a baseline guard before ordinary, Program, and retry dispatch. `ensure_clean_baseline_before_dispatch` checks workflow canonicalization commands, records private events, serializes normalization with `.decodex-baseline-normalization.lock`, may create/land a baseline normalization PR, and blocks if canonicalization still rewrites tracked files (`apps/decodex/src/orchestrator/baseline_guard.rs`). This came from recent commits titled "Guard baseline canonicalization before dispatch" and should be considered part of current dispatch safety.

## Long-running control plane

`decodex serve` calls `orchestrator::run_control_plane` through `apps/decodex/src/cli/control_commands/serve.rs`. The operator listener default is `127.0.0.1:8192` in README examples and operator docs. `--dev` is hidden and is only for isolated endpoint testing; it does not represent normal scheduling.

Each daemon tick (`apps/decodex/src/orchestrator/daemon.rs`):

- reconciles active child process state and retry queue entries
- recovers and reconciles idle project state when no active children exist
- reconciles post-review orchestration
- reconciles terminal thread archive backlog
- spawns due child attempts until no more can start

`openwiki/workflows/runtime-operator-workflows.md` records the current cadence: operator snapshots publish every 15 seconds, and Linear-backed queue/status scans run at most every 5 minutes per project unless `POST /api/linear-scan` requests a scan.

## App-server execution

`apps/decodex/src/agent/app_server/run.rs` owns direct `codex app-server` execution. One attempt:

1. Records run attempt status as `starting`.
2. Writes activity markers when configured.
3. Publishes a run-control channel for lane control.
4. Spawns `codex app-server` through the JSON-RPC client.
5. Initializes the client and records user-agent/capability evidence.
6. Runs capability preflight and optional `command/exec` health check.
7. Logs into a selected Codex account when account-pool routing is configured.
8. Starts or resumes a thread session.
9. Records `running`, executes the turn loop, then records `succeeded`.
10. Retires the run-control channel as completed or failed.

`openwiki/specs/contracts-and-data.md` summarizes protocol requirements: Decodex uses `stdio://`, expects generated-schema compatibility, requires phase-goal methods, exposes issue-scoped dynamic tools, and treats `decodex probe stdio://` with `PROBE_OK` as a live compatibility check.

## Tracker bridge and completion

The issue-scoped tracker bridge in `apps/decodex/src/agent/tracker_tool_bridge.rs` binds the agent to one leased issue. It exposes dynamic tool names such as `issue_transition`, `issue_comment`, `issue_label_add`, `issue_progress_checkpoint`, `issue_review_checkpoint`, `issue_review_handoff`, `issue_review_repair_complete`, `issue_closeout_complete`, and `issue_terminal_finalize`.

The important architecture boundary is not the tool list itself; it is who owns what:

- The agent may perform bounded issue-scoped tracker writes through dynamic tools.
- The runtime still owns leases, worktrees, retries, recovery, crash fallback, post-review lifecycle, and cleanup.
- Private evidence goes into runtime SQLite before any public Linear projection when a tool has both private and public effects.
- Terminal completion must be explicit; the runtime should not guess whether a lane meant review handoff, manual attention, repair completion, or closeout.

## Operator HTTP and dashboard

`apps/decodex/src/orchestrator/operator_http.rs` owns the local HTTP endpoint, dashboard assets, API routes, and WebSocket/control traffic. The route module exports dashboard pages, `/livez`, operator snapshot APIs, account APIs, app snapshot, Linear scan, lane inspect, lane interrupt, and lane steer endpoints. HTTP and WebSocket readbacks are projections over runtime state; they must not become separate lifecycle authority.

## MCP gateway

`apps/decodex/src/mcp.rs` serves MCP over stdio or Streamable HTTP:

- Stdio defaults to `admin` capability profile.
- Streamable HTTP defaults to `observe`, binds to `127.0.0.1:8193`, serves `POST /mcp`, validates origins, manages `Mcp-Session-Id`, and requires bearer auth for non-loopback or profiles above observe (`apps/decodex/src/mcp.rs`, `openwiki/workflows/runtime-operator-workflows.md`).
- Tool profiles are `observe`, `plan`, `operate`, and `admin`.
- Tools include `decodex_observe`, `decodex_plan`, goal/autonomy planning tools, `decodex_lane_control`, and `decodex_project_control` (`apps/decodex/src/mcp.rs`).

MCP is a typed facade over existing runtime and operator controls. It is not a bypass around Decision Contract acceptance, lane-control preconditions, tracker boundaries, review policy, or project enablement.

## Change guidance

- CLI changes: start in `apps/decodex/src/cli.rs` and the owning submodule under `apps/decodex/src/cli/`; add parser tests under `apps/decodex/src/cli/tests/`.
- Runtime scheduling changes: start in `apps/decodex/src/orchestrator/run_cycle.rs`, `apps/decodex/src/orchestrator/daemon.rs`, and the lifecycle-specific orchestrator submodule; expect dense tests under `apps/decodex/src/orchestrator/tests/`.
- State changes: start in `apps/decodex/src/state/sqlite_store/schema.rs`, migrations, row parsers, and `StateStore`; protect replay/idempotency with state tests.
- App-server changes: start in `apps/decodex/src/agent/app_server/`; run app-server schema/probe tests and avoid relying only on stale handwritten protocol notes.
- Operator/MCP changes: update HTTP/MCP tests and check public/private projection boundaries before exposing new fields.

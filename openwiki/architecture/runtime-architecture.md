# Runtime Architecture

This page explains the active vNext ownership skeleton and preserves a map of the
excluded v0.2 source for provenance. Checked-in manifests, source, tests, and the vNext
authority documents remain authoritative.

## Workspace shape

The root manifest enumerates the active members explicitly. Five library owners form the
vNext runtime boundary:

- `decodex-core`: pure domain/application contracts and ports; no dependencies.
- `decodex-protocol`: V1 typed wire contracts, current/previous-minor negotiation, and
  loopback endpoint policy; depends only on core plus structured serialization.
- `decodex-postgres`: the PostgreSQL 18 product-state adapter; depends on core plus the
  accepted tokio-postgres/deadpool/refinery stack and owns embedded migrations,
  optimistic transactions, leases, append-only activity, outbox delivery, and inert
  account/window metadata.
- `decodex-codex`: typed app-server contracts, schema/live capability negotiation,
  redacted event normalization, and immutable one-account process supervision. Live
  turn dispatch remains unavailable while XY-1304 is failed.
- `decodex-runtime`: service lifecycle, connection/session execution, resumable event
  publication, idempotency receipts, and adapter composition; depends on the other four
  owners plus the maintained Axum/Tokio transport stack.

`apps/decodexd` depends only on runtime. The `apps/decodex-cli` and
`apps/decodex-gpui` client roots depend only on protocol, so they cannot reach stores,
Codex, repositories, or orchestration directly. Radar and Publisher remain independent
auxiliary workspace members. `tests/scripts/test_vnext_architecture.py` checks the exact
dependency graph and exclusion of the legacy package through Cargo metadata.

`decodexd` is the only V1 server composition root. It binds
`ws://127.0.0.1:49152/v1/ws`, and the endpoint type refuses every non-loopback address
before opening a socket. The single physical WebSocket uses structured JSON and typed
hello, command, receipt, result, snapshot, event, and refusal envelopes. Major versions
must match exactly; this build accepts minors 1 and 0. Events carry server ID, monotonic
cursor, entity revision, correlation, and causation. A reconnect to the same server ID
resumes retained ordered deltas; a stale cursor or changed server ID receives a bounded
snapshot fallback. Only a snapshot or event cursor fully applied by the client is a
resume checkpoint; the Welcome cursor is an informational server high-water mark and
must not advance client progress before following replay deltas are applied.

Runtime idempotency is keyed by the command's idempotency key plus its typed payload and
optional expected revision. A duplicate returns the original command identity and the
same stored result without a second application execution; conflicting reuse is
rejected. The receipt ledger has a fixed lifetime capacity: accepted keys are never
evicted, duplicates remain readable at capacity, and new keys are refused before
application execution once full. Replay buffers, snapshot item counts, human-readable
wire scalars, inbound and outbound message sizes, writes, and per-client outbound queues
are bounded. Queue overflow disconnects that client with WebSocket close code 1013;
an oversized outbound frame closes with code 1009. A successful application publication
is retained even if its initiating client overflows, so cursor resume cannot lose the
mutation. Once accepted, command execution is shielded from connection-task cancellation;
a disconnect can suppress delivery but cannot cancel receipt/event recording. Snapshot
types contain only small state and cannot carry artifact bytes.

This slice deliberately keeps its replay/idempotency ledger in memory. A daemon restart
changes server identity, loses that transient ledger, and forces snapshot fallback;
durable PostgreSQL product-state and transaction primitives live in `decodex-postgres`,
but the active composition supplies no connection until XY-1268 owns bootstrap and path
integration. The foundation application therefore reports product state unavailable
rather than inventing product entities. PostgreSQL reports available only after an
explicit connection passes PostgreSQL 18, checksum, extension, and migration checks.
The composition also reports conversation execution unavailable. Authentication, TLS,
remote binding, HTTP artifact transfer, MCP, scheduling, live Codex execution, CLI
operations, and GPUI behavior remain disabled and belong to later issues.

The synchronous product-state availability port reflects verified configuration and local
pool lifecycle: closing the pool makes it unavailable. Live PostgreSQL failures remain
authoritative at each asynchronous store operation; availability does not claim a synchronous
network-liveness probe.

The store's owned mutations reserve a command receipt, apply an exact expected revision,
append activity, enqueue the matching outbox effect, and complete the receipt in one
transaction. Reusing the same idempotency key and request hash reads the committed
response; reusing the key for different bytes is rejected. Outbox claims are bounded and
fenced by a token rotated on every claim or reclaim. Any effect that may have begun must be
reconciled through a meaningful receipt and authoritative readback after claim expiry or
restart. Lease, retry, and retention durations are exact positive whole milliseconds capped
at 365 days; stored lease functions enforce the same fixed-millisecond boundary, lease rows
cannot exceed it relative to their update time, and in-flight outbox leases carry a persisted
claim-or-renewal timestamp anchor. Delivered outbox retention is finite, positive,
whole-millisecond, chronological, and capped at 365 days. Delivered rows are terminal and
immutable until retention pruning is due; no other outbox state is deletable, and table
truncation is forbidden. Direct SQL therefore cannot delete and recreate a completed external
effect as replayable work.
Operation-time triggers reject caller-shifted anchors and
deadlines beyond the same 365-day horizon; relative-duration `CHECK` constraints remain
wall-clock independent. Quota mutation responses and command receipts use PostgreSQL's
persisted, microsecond-rounded UTC timestamps rather than caller timestamp text. Account and
quota-window rows are
inert observations with recursive credential-material rejection across normalized keys and
recognizable secret-bearing value encodings. PostgreSQL explicitly normalizes Rust's full
Unicode `White_Space` set, applies an explicit ASCII case fold, and evaluates the remaining
case-sensitive regular expressions under the built-in `C` collation. The integration gate
repeats credential vectors in a Turkish ICU database so database-default case rules cannot
weaken the direct-SQL boundary; this crate exposes no
eligibility, account selection, fallback, wake scheduling, or credential storage.

## Active Codex adapter foundation

`crates/decodex-codex/` structurally extracts request/notification methods from generated
schema, validates native-collaboration object variants, required fields, field types, and
referenced enums from `ThreadReadResponse`, and compares canonical JSON digests with the
accepted XY-1262 receipt before spawning an app server.
Marker presence is only schema evidence: live method outcomes become explicit `supported`,
`unsupported`, `unavailable`, or `degraded` states. Every successful probe must enter its
profile into the exact-build cache, which rejects conflicting replacement and has no
nearest-build or stale fallback.

Each `SupervisedProcess` owns one immutable shared-home account authority. Credential
environment variables are removed from the child, initialize must report the normal
`$HOME/.codex`, no credential-switch operation exists, and read-only `account/read` must
establish a pseudonymous active-account receipt before a successful probe. On Unix the child starts in a new session;
bounded shutdown checks the entire process group independently of the leader and escalates
from termination to kill. Raw JSON-RPC, stderr, account details, and free-form model deltas
stay inside the adapter. Callers receive typed probe results, stable errors, correlation-
only message events, and run-local collaboration actors identified by validated UUIDs or
digests rather than optional nickname/role fields. Build identity is an exact opaque
fingerprint; statuses, activity kinds, tools, capability reasons, and read-only methods
are closed enums, so protocol text is not exported through debug or serialization paths.

The production command resolves `codex` once to a canonical absolute executable, reads it
through a fixed byte limit, includes its digest in the opaque build identity, and rechecks
the same path and digest before version, schema, and app-server spawn. Only tests can inject
a fake executable. Preflight output/files, generated-schema file count/per-file/aggregate
bytes/depth, inbound and outbound app-server frames, the stdout queue, collaboration
receiver count, and thread-list/search results are mechanically bounded; schema traversal
rejects symlinks and special files. Both preflight commands use bounded process-group
supervision and descendant cleanup. Failed bounded cleanup transfers the still-owned child
and process group to a persistent reaper rather than relinquishing process authority.

The probe sends `initialize`, `initialized`, read-only `account/read`, bounded
`thread/list(useStateDbOnly=true)`, exact-ID `thread/read(includeTurns=false)` when a listed
thread exists, and `thread/search` with a fixed nonmatching term and bounded result count.
The latter two calls establish method availability only; they do not claim global title
discovery. Account identity is pseudonymized and re-attested after every authority read;
an identity change discards the in-progress negotiation, and restart retains the expected
identity for re-attestation. Archive, paginated persisted history, and native collaboration
remain explicitly `not_probed` while their side-effect/event gates are closed. Paginated
history schema evidence comes from the structural `ThreadStartParams.historyMode` /
`ThreadHistoryMode` contract, never from a list cursor.
`DispatchGate` has no enabled state and denies thread start/resume, turn start/steer/
interrupt, and approval responses under XY-1304. The composition root therefore still
reports conversation execution unavailable even though schema validation, read-only
probing, normalization, and supervision are implemented.

## Frozen v0.2 runtime provenance

Everything below this heading maps the preserved `apps/decodex/` source. That package
is excluded from the active Cargo workspace and is not a runtime, fallback, compatibility
facade, or implementation authority for vNext.

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

`decodex serve` calls `orchestrator::run_control_plane` through `apps/decodex/src/cli/control_commands/serve.rs`. The operator listener default is `127.0.0.1:8192` in README examples and OpenWiki operator notes. `--dev` is hidden and is only for isolated endpoint testing; it does not represent normal scheduling.

Each daemon tick (`apps/decodex/src/orchestrator/daemon.rs`):

- reconciles active child process state and retry queue entries
- recovers and reconciles idle project state when no active children exist
- reconciles post-review orchestration
- reconciles terminal thread archive backlog
- spawns due child attempts until no more can start

`openwiki/workflows/runtime-operator-workflows.md` records the current cadence: operator snapshots publish every 15 seconds, and Linear-backed queue/status scans run at most every 5 minutes per project unless `POST /api/linear-scan` requests a scan.

## Frozen app-server execution

The excluded v0.2 `apps/decodex/src/agent/app_server/run.rs` owned direct live
`codex app-server` execution. It is provenance only and must not be used by vNext. One
legacy attempt:

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

`apps/decodex/src/orchestrator/operator_http.rs` owns the local HTTP endpoint, dashboard support constants, API routes, and WebSocket/control traffic. Routes are parsed in `apps/decodex/src/orchestrator/operator_http/routes.rs` and dispatched by `apps/decodex/src/orchestrator/operator_http/server.rs`: `/livez`, `/api/operator-snapshot`, `/dashboard/control`, `/api/accounts`, `/api/linear-scan`, `/api/lane/inspect`, `/api/lane/interrupt`, and `/api/lane/steer`/`/api/lane-steer`. `apps/decodex/src/orchestrator/operator_http/assets.rs` defines the dashboard/client limits, HTTP read timeout, and volatile run-activity fingerprint fields.

The published dashboard snapshot is a cached JSON serialization of `OperatorStatusSnapshot` protected by the `OperatorStateEndpoint` mutex; `publish_snapshot` is the only writer and broadcasts a `snapshot` WebSocket event after updating it (`apps/decodex/src/orchestrator/types/operator_endpoint.rs`). `/api/operator-snapshot` and the dashboard WebSocket read from that cache, inject live global account-control state, and add presentation fields (`apps/decodex/src/orchestrator/operator_http/snapshot.rs`). Runtime lifecycle state still belongs to SQLite, leases, worktrees, tracker ledgers, and daemon/recovery code; HTTP and WebSocket paths are readback/control projections, not new ownership stores.

Dashboard streaming has two readback paths. `snapshot` events follow control-plane publication, normally once per 15-second tick; `runActivity` events are a lighter 1-second stream that rebuilds current-lane projections without live external observers, strips volatile timing fields from its fingerprint, suppresses duplicate broadcasts, and can be filtered by project, issue, or run subscription (`apps/decodex/src/orchestrator/constants.rs`, `apps/decodex/src/orchestrator/operator_http/dashboard/run_activity.rs`, `apps/decodex/src/orchestrator/operator_http/dashboard/subscription.rs`). Dashboard control messages are intentionally narrow: focus/subscription changes are session-local, acknowledgements are session-local, and account selection only changes the global Codex account selector through the accounts module (`apps/decodex/src/orchestrator/operator_http/dashboard/control_actions.rs`).

Account HTTP APIs delegate to account-domain commands: list with optional usage refresh, select, clear, logout, import, use, and reroll name (`apps/decodex/src/orchestrator/operator_http/api/account.rs`). Lane inspect, interrupt, and steer delegate to `lane_control` with registered-project resolution and keep audit/control effects in runtime state (`apps/decodex/src/orchestrator/operator_http/api/lane.rs`). Private execution evidence is read back through `decodex evidence`, where `build_private_evidence_readback` loads local SQLite private events, summarizes payloads by default, and emits stable `private-evidence:<project>/<issue>/<run>/<attempt>` references (`apps/decodex/src/orchestrator/agent_evidence/private_readback.rs`); dashboard/operator status may show the reference and read command, but raw private payloads are not a public dashboard contract.

Linear-facing updates stay sparse. The progress checkpoint tool first records the full execution-state checkpoint as private runtime evidence, then publishes only a low-frequency public Linear projection when the public lifecycle signal/idempotency key changes (`apps/decodex/src/agent/tracker_tool_bridge/tools/progress_checkpoint/handler.rs`, `apps/decodex/src/agent/tracker_tool_bridge/tools/progress_checkpoint/projection.rs`). Control-plane Linear scans run at most every 5 minutes per project unless `/api/linear-scan` queues an all-project or project-scoped scan for the next tick; that request still waits behind active tracker connector backoff (`apps/decodex/src/orchestrator/entrypoints_control_plane.rs`, `apps/decodex/src/orchestrator/entrypoints_control_plane/project_tick.rs`, `apps/decodex/src/orchestrator/operator_http/api/linear_scan.rs`). Rate-limit and timeout detection are modeled as connector backoff warnings (`tracker_rate_limited`, `tracker_transient_timeout`), persisted in runtime state, included in snapshots, and used to skip external observer reads until retry is safe (`apps/decodex/src/orchestrator/entrypoints_tracker_backoff.rs`, `apps/decodex/src/orchestrator/status/snapshot/observers/backoff.rs`).

Current non-goals are important boundaries: the operator HTTP server is loopback-local by default, not a durable workflow engine; dashboard focus/ack messages are not lifecycle events; account controls affect future account selection but do not rewrite existing lane authority; `/api/linear-scan` requests observation rather than dispatch; and lane steering/interrupt APIs must continue to honor lane-control preconditions, leases, private audit evidence, and tracker/public-text privacy boundaries.

## MCP gateway

`apps/decodex/src/mcp.rs` serves MCP over stdio or Streamable HTTP:

- Stdio defaults to `admin` capability profile.
- Streamable HTTP defaults to `observe`, binds to `127.0.0.1:8193`, serves `POST /mcp`, validates origins, manages `Mcp-Session-Id`, and requires bearer auth for non-loopback or profiles above observe (`apps/decodex/src/mcp.rs`, `openwiki/workflows/runtime-operator-workflows.md`).
- Tool profiles are `observe`, `plan`, `operate`, and `admin`.
- Tools include `decodex_observe`, `decodex_plan`, goal/autonomy planning tools, `decodex_lane_control`, and `decodex_project_control` (`apps/decodex/src/mcp.rs`).

MCP is a typed facade over existing runtime and operator controls. It is not a bypass around Decision Contract acceptance, lane-control preconditions, tracker boundaries, review policy, or project enablement. The design rationale for keeping MCP typed and skills slim lives in [Design rationale](../decisions/design-rationale.md); the current remote-control drift audit lives in [Drift audits](../evidence/drift-audits.md).

## Change guidance

- CLI changes: start in `apps/decodex/src/cli.rs` and the owning submodule under `apps/decodex/src/cli/`; add parser tests under `apps/decodex/src/cli/tests/`.
- Runtime scheduling changes: start in `apps/decodex/src/orchestrator/run_cycle.rs`, `apps/decodex/src/orchestrator/daemon.rs`, and the lifecycle-specific orchestrator submodule; expect dense tests under `apps/decodex/src/orchestrator/tests/`.
- State changes: start in `apps/decodex/src/state/sqlite_store/schema.rs`, migrations, row parsers, and `StateStore`; protect replay/idempotency with state tests.
- vNext app-server foundation changes: start in `crates/decodex-codex/`; run its schema,
  fake-process, supervision, redaction, and dispatch-guard tests. The excluded
  `apps/decodex/src/agent/app_server/` is provenance only.
- Operator/MCP changes: update HTTP/MCP tests and check public/private projection boundaries before exposing new fields.

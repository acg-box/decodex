---
type: "Spec"
title: "App-Server Specification"
description: "Define the direct `codex app-server` protocol boundary used by the `decodex` MVP. Status: normative Read this when: You are implementing or validating `decodex`'s direct `codex app-server` integration, including transport, handshake, request flow, or dynamic tools. Not this document: The runtime state machine, downstream `WORKFLOW.md` policy, or operator runbooks. Defines: The supported transport, protocol source-of-truth boundary, required request and notification flow, the MVP contract for `initialize`, `thread/start`, and repeated `turn/start` calls on one thread, and the narrow health-check use of standalone `command/exec`."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/agent/app_server.rs, apps/decodex/src/agent/app_server/protocol.rs, apps/decodex/src/agent/app_server/tests.rs, apps/decodex/src/agent/tracker_tool_bridge.rs]
related: [./tracker-tools.md, ./lane-control.md, ./runtime.md]
drift_watch: ["codex app-server generate-json-schema --experimental", "decodex probe stdio://", ThreadStartParams.dynamicTools, dynamicTools, "type:function", "type:namespace", ClientRequest, ServerRequest, ClientNotification, ServerNotification, "account/rateLimitResetCredit/consume", "externalAgentConfig/import/readHistories", "thread/realtime/appendSpeech", "externalAgentConfig/import/progress"]
last_verified: 2026-06-19
---
# App-Server Specification

Purpose: Define the direct `codex app-server` protocol boundary used by the `decodex` MVP.
Status: normative
Read this when: You are implementing or validating `decodex`'s direct `codex app-server` integration, including transport, handshake, request flow, or dynamic tools.
Not this document: The runtime state machine, downstream `WORKFLOW.md` policy, or operator runbooks.
Defines: The supported transport, protocol source-of-truth boundary, required request and notification flow, the MVP contract for `initialize`, `thread/start`, and repeated `turn/start` calls on one thread, and the narrow health-check use of standalone `command/exec`.

## Transport

- The MVP transport is `stdio://`.
- `decodex` starts the child process with:

```sh
codex app-server --listen stdio://
```

- Before launching the child, `decodex` resolves the shared Codex home as
  `$HOME/.codex`. Missing, empty, or relative `HOME` is a dispatch preflight
  failure.
- `decodex` clears any previously prepared child-process values for `CODEX_HOME`
  and `CODEX_SQLITE_HOME`, then sets both variables to the resolved shared Codex
  home. Account selection must not choose or mutate these homes.
- `ws://` is out of scope for the MVP.

## Source of truth

- The generated JSON Schema bundle is the authoritative local protocol source.
- Generate the bundle with:

```sh
codex app-server generate-json-schema --experimental --out target/decodex-app-server-schema-check
```

- `decodex` must treat the generated schema as more authoritative than stale handwritten assumptions.
- `--experimental` is required when inspecting `dynamicTools` and related experimental fields in the generated bundle.

## Protocol support evidence

The current Decodex app-server support contract is capability-gated rather than a broad
"latest Codex" promise. A Codex app-server surface is usable only when all of these
are true:

- `codex app-server generate-json-schema --experimental` succeeds.
- The generated schema contains the Decodex-owned request and notification contract in
  this spec, including `initialize`, `thread/start`, `thread/resume`, `turn/start`,
  `thread/archive`, `command/exec`, the bounded preflight methods, `item/tool/call`,
  dynamic tool `type:function`, dynamic tool `type:namespace`, namespace
  `tools[]`, dynamic tool `deferLoading`, `inputText` tool responses, and
  `PluginListParams.marketplaceKinds`.
- `decodex probe stdio://` completes the app-server capability preflight,
  standalone `command/exec` health check, and dynamic-tool round trip with
  `PROBE_OK`.

Support evidence has two distinct layers:

- Capability evidence: the bounded runtime preflight records the app-server methods
  and inventories Decodex actually checked before `thread/start` or `thread/resume`.
  Any required capability failure is a pre-dispatch app-server preflight blocker
  rather than a promptable agent turn.
- Schema evidence: `decodex probe stdio://` regenerates the local schema cache and
  checks the required markers, the Decodex-owned JSON-RPC method unions, and the
  dynamic-tool declaration shape in this spec before completing the dynamic-tool
  round trip. Normal retained dispatch does not regenerate the schema cache.

`decodex probe stdio://` reports the probe result with `preflight_checks`, `thread`,
`turn`, `events`, and `output`. A passing probe must include `output=PROBE_OK`.

### 0.141 preview drift audit

The 2026-06-18 audit compared Codex app-server `0.140.0` with
`0.141.0-alpha.7`.

- No JSON-RPC method was removed.
- Added client requests were `account/rateLimitResetCredit/consume`,
  `externalAgentConfig/import/readHistories`, and
  `thread/realtime/appendSpeech`.
- Added server notification was `externalAgentConfig/import/progress`.
- Decodex's owned orchestration schemas kept the same top-level required fields.
- `ThreadStartParams.dynamicTools` changed from the legacy flat
  `dynamicTools[].namespace` shape to the tagged `type:function` /
  `type:namespace` union below. That shape is Decodex-owned because retained runs
  expose issue-scoped dynamic tools.

The newly added account-credit, external-import, and realtime-speech surfaces are
Codex product APIs, not Decodex retained-run orchestration APIs. Decodex must not
add no-op wrappers for them merely to mirror the full app-server surface. If a
future Decodex feature depends on one, promote it into the owned-method union and
add an explicit runtime preflight or live probe for that feature.

Phase-scoped goal support is mandatory and capability-gated for retained lane
execution. App-server surfaces must expose `thread/goal/set`, `thread/goal/get`,
`thread/goal/clear`, and `thread/goal/updated`. Decodex rejects old or incompatible
app-server builds that lack these methods with a typed unsupported-app-server blocker.
It must not fall back to ordinary continuation, and it must not reject newer, beta,
alpha, or unknown app-server versions solely because of the version string when the
required goal methods work.

Goal events are phase signals only. A `complete` goal status triggers Decodex-owned
validation or handoff policy; it never satisfies terminal issue completion by itself.
For implementation and repair phases, agents must complete the active phase goal
when the local validation-ready objective is satisfied. Progress checkpoints and final
text such as "await next phase" remain evidence only; Decodex advances to repo-gate
validation and the next phase only from the explicit goal-complete signal.

To validate an upstream app-server protocol change:

1. Install or select the target Codex binary locally without disrupting active lanes.
2. Run `codex app-server generate-json-schema --experimental --out
   target/decodex-app-server-schema-check`.
3. Confirm the generated schema contains every required marker in this spec:
   `initialize`, `thread/start`, `thread/resume`, `turn/start`, `thread/archive`,
   `thread/goal/set`, `thread/goal/get`, `thread/goal/clear`,
   `thread/goal/updated`, `command/exec`, bounded preflight methods,
   `item/tool/call`, dynamic tool `type:function`, dynamic tool `type:namespace`,
   namespace `tools[]`, dynamic tool `deferLoading`, `inputText`, and
   `PluginListParams.marketplaceKinds`.
4. Confirm the generated `ClientRequest`, `ServerRequest`, `ClientNotification`, and
   `ServerNotification` method unions still contain Decodex-owned methods with their
   expected params schema names.
5. Run `decodex probe stdio://` and require `PROBE_OK`.
6. Update this spec or the runtime preflight only when the protocol shape or required
   capability set changes.

## Implementation guidance

- When implementing Decodex features that depend on Codex runtime behavior, read the relevant Codex or `app-server` implementation path, not only this contract.
- This is especially required for features such as idle timeout policy, stall detection, retry boundaries, waiting-state handling, and any other liveness-sensitive behavior.
- Use this document to constrain protocol shape, then use the upstream implementation to adapt Decodex behavior to how Codex actually emits progress, waits, and terminates turns.
- Do not finalize these features from local heuristics alone when the upstream runtime behavior can be inspected directly.

## Upstream alignment

- Upstream Symphony remains the ownership reference for the orchestration boundary.
- `decodex` keeps one deliberate contract divergence here: TOML frontmatter in downstream `WORKFLOW.md`.
- For the next phase, the preferred tracker-tool transport is a client-side dynamic tool bridge handled inside the existing JSON-RPC client.
- Rationale: the local generated schema already exposes server-driven dynamic tool call requests (`item/tool/call`) and related tool-call notifications, so `decodex` can service issue-scoped tracker writes without introducing a second child service for the first dogfood pilot.
- A process-local MCP server remains a future option if the tool surface expands or if the dynamic bridge proves too constrained.

## Protocol shape

- Protocol family: JSON-RPC request/response plus asynchronous notifications.
- Required client requests for the MVP:
  - `initialize`
  - `config/read`, `model/list`, `modelProvider/capabilities/read`,
    `skills/list`, `plugin/list`, and `mcpServerStatus/list` for bounded
    capability preflight after `initialize` and before thread dispatch
  - `command/exec` for bounded app-server health checks only
  - `thread/start`
  - `thread/resume` when retrying a persisted same-thread continuation
  - `thread/goal/set`
  - `thread/goal/get`
  - `thread/goal/clear`
  - `turn/start`
  - `thread/archive` after successful completion writeback, for every locally
    recorded terminal attempt thread on the issue that has not already recorded a
    terminal archive event (`thread/archive` or `thread/archive/discarded`)
- Required notifications for the MVP:
  - `thread/started`
  - `thread/status/changed`
  - `thread/goal/updated`
  - `turn/started`
  - `turn/completed`

Additional notifications may be recorded opportunistically for diagnostics.
`thread/goal/updated` is recorded as local protocol activity and may summarize the
active phase and status for operator readback. It is not a public tracker signal.

Retrying app-server `error` notifications are nonterminal. When an error payload
sets `willRetry: true`, `decodex` must keep waiting for the same turn instead of
treating the notification as the latest terminal turn failure. A later terminal
`turn/completed` error, a non-retrying `error`, or the idle timeout may still end
the attempt. Model-side `item/started` notifications keep the turn in the
`model_execution` waiting state for liveness timeout selection; tool and command
item starts must not extend model-execution idle handling.
After `thread/archive` or `thread/archive/discarded` is recorded for a run, Decodex
treats later non-terminal app-server events for that run as late diagnostics rather
than authoritative progress. Those events are discarded into the runtime journal's
post-archive namespace so they cannot collide with the archive sequence, cannot
replace the archive as the terminal protocol marker, and cannot make parent
journal/closeout recovery consume the child retry budget.

The follow-up alignment phase should also record tool-related requests and notifications needed for issue-scoped tracker writes.

Decodex records a compact local protocol summary from high-value structured
notifications instead of scraping transcripts. The summary may include
`turn/started`, `turn/completed`, plan updates, diff updates, item
start/completion, command output deltas, server request responses, account updates,
rate-limit updates, warning/deprecation notices, model reroutes/verifications, and
thread token-usage updates. This summary is published through the operator status
snapshot and dashboard only; high-frequency protocol details remain out of Linear
unless an existing lifecycle event summarizes them.

Lane-control protocol methods are an additive operator-control extension, not part of
the current normal dispatch preflight. Decodex's intended lane-control use is:

- `turn/interrupt` for soft active-turn interruption when the active turn id is known.
- `turn/steer` for broad operator-supplied steering text.
- no operator-facing `thread/inject_items` feature in this rollout.

If generated schema or live capability probing shows that `turn/interrupt` or
`turn/steer` is unavailable, the CLI/API control must report that control as
unsupported for the active lane instead of failing ordinary issue dispatch. The
lane-control contract and support matrix live in [`lane-control.md`](./lane-control.md).
Decodex currently implements `turn/interrupt` and `turn/steer` through the child-owned
app-server connection for active turns.

## Required request flow

1. Start the child process.
2. Send `initialize`.
3. Run the bounded capability preflight with `config/read`, `model/list`,
   `modelProvider/capabilities/read`, `skills/list`, `plugin/list`, and
   `mcpServerStatus/list`.
4. When `[codex.accounts]` is enabled, select a shared ChatGPT account and send
   `account/login/start` with `chatgptAuthTokens`.
5. Send `thread/start`.
6. Send `thread/goal/set` with the controller-owned phase goal for the run.
7. Send `turn/start`.
8. Consume notifications until that turn reaches a terminal outcome.
   If the `turn/start` response id and same-thread notification turn id differ,
   Decodex treats the notification turn id as the active turn id for subsequent
   item, goal, completion, and lane-control readbacks.
9. Send `thread/goal/get` after the turn completes. If the goal status is `complete`,
   Decodex runs the next owned phase transition such as repository validation,
   validation repair, review repair, or handoff evidence. If the goal remains active
   and bounded continuation is allowed, Decodex may start another turn on the same
   thread. If a required goal method is missing, Decodex fails the run with the typed
   unsupported-app-server reason instead of continuing without a goal.
10. If the project-owned continuation policy allows another same-thread turn, send
   another `turn/start` on the same thread.
11. Persist the local run journal and classify the bounded run result.
12. After successful completion writeback, best-effort archive all locally recorded
    terminal attempt threads for the issue so prior failed retry attempts do not keep
    the Codex conversation list visible.

The capability preflight is observational. It may inspect the effective app-server
config, model inventory, provider capabilities, skill inventory, plugin inventory,
and MCP server state, but it must not install plugins, mutate marketplaces, or send
model, personality, sandbox, or approval-policy overrides on behalf of
`WORKFLOW.md`. `plugin/list` preflight must pass `marketplaceKinds = ["local"]`
so remote catalog, featured-plugin, or marketplace-discovery failures do not gate a
business lane before its thread is created.
`skills/list` scan errors are diagnostics when the response still includes the run
cwd and at least one enabled skill. Decodex must preserve the scan error count and
first error details in local preflight evidence, but it must not block the lane solely
because unrelated installed skill metadata failed to scan.
Because `plugin/list` is observational and local-marketplace-only, Decodex may retry
one app-server output timeout inside the preflight request before failing that request.
If the preflight request retry is exhausted, the lane must still enter structured
runtime retry while workflow retry budget remains. Only after workflow retry budget
exhaustion may the terminal failure become operator-facing attention; when that
happens it must remain an app-server preflight failure, report
`app_server_plugin_list_timeout`, and include the `plugin/list` timeout cause in local
preflight evidence and operator recovery output rather than looking like a repository
implementation failure.

When dynamic tools are enabled, `decodex` must also:

1. Register the tool surface in `thread/start.dynamicTools` using the 0.141 tagged
   schema: unnamespaced tools are `type:function` specs, and Decodex namespaced
   tools are grouped into `type:namespace` specs whose `tools[]` entries are
   nested `type:function` specs.
2. Answer `item/tool/call` requests with `DynamicToolCallResponse`.
3. Serialize dynamic tool output items with schema-approved `type` values such as `inputText`.
4. Keep every dynamic tool name and namespace name within the app-server identifier
   pattern `^[a-zA-Z0-9_-]+$`.
5. Validate incoming `item/tool/call` thread, tool-name, namespace, and response shape before treating the request as handled. Dynamic tool request `turnId` is diagnostic context only: the app-server may emit a request-scoped turn id that differs from the active `turn/start` id, so Decodex records mismatches but keeps the authorization boundary on the active thread and declared tool surface.

The client-side dynamic bridge may expose narrow Decodex-owned tools that are local to one run attempt, such as the deferred `decodex.decodex_run_context` tool. These tools must stay small and side-effect-bounded so they can move to a process-local MCP server if the surface expands. Broader stateful or cross-service tool families remain MCP candidates rather than reasons to grow the client bridge indefinitely.

If app-server sends an invalid or undeclared `item/tool/call`, `decodex` must respond with a failed `DynamicToolCallResponse`, record an operator-local `item/tool/call/failure` diagnostic with a normalized failure class and next action, and fail the run as an app-server dynamic-tool protocol failure. If a declared Decodex tool returns `success = false`, `decodex` records the same local diagnostic but leaves the turn alive so the model can correct arguments or backing state within the same run.

When `[codex.accounts]` is enabled, the account pool is a global Decodex file at
`~/.codex/decodex/accounts.jsonl`; project configs do not own an account-pool path
override. The pool accepts flat `auth.json`-style JSONL records or records wrapped as
`{ "auth": ... }`.
Before login, Decodex probes configured accounts through the ChatGPT usage endpoint.
By default, it skips disabled, cooling-down, and incomplete records, penalizes
usage-limited records, scores both the short primary window and the longer secondary
window, prefers the account with the strongest remaining bottleneck capacity, and uses
the least-recently selected account to break equal capacity scores. If the global
`~/.codex/decodex/config.toml` sets `[codex.accounts].fixed_account`, Decodex only
considers the matching account instead of balancing across the pool. The selector
matches an account email, full account id, or redacted account fingerprint as displayed
in the operator UI. Project configs can enable account-pool use, but they do not own a
project-scoped fixed account.
Successful selection writes `last_selected_at_unix_epoch` back to the JSONL file, and
selection holds a pool-local lock so concurrent run dispatches observe the latest
selector state instead of all choosing from the same stale snapshot.
If token refresh returns an authentication or authorization rejection, Decodex writes
`auth_failed_at_unix_epoch` and `auth_failure` to the matching JSONL record, excludes
that account from later selection, reports `status = "auth_failed"` in account
snapshots, and fails any active lane with `codex_account_auth_failed` instead of
retrying through no-diff or contract-boundary recovery.
If a turn later fails with `codexErrorInfo = "usageLimitExceeded"`, Decodex treats it
as a retryable capacity failure while retry budget remains. The current turn stops
immediately, but the next attempt re-enters normal account selection so the pool can
refresh usage and choose another usable account. Only retry-budget exhaustion makes
that error a human-required `app_server_usage_limit_exceeded` stop.

Decodex owns token freshness for injected `chatgptAuthTokens`. It proactively refreshes
an account before probing when the access-token JWT `exp` is expired. If no expiration
claim is available, it refreshes when `last_refresh` is more than eight days old. If
the app-server later sends `account/chatgptAuthTokens/refresh`, Decodex refreshes the
globally fixed account when configured, otherwise the previous account id supplied by the
request. It updates the JSONL record with returned tokens and `last_refresh`, records
a redacted local protocol event, and responds with fresh `chatgptAuthTokens`. When the
same account is currently active in the Codex `auth.json` target, Decodex also mirrors
the refreshed token payload there so the standalone Codex CLI does not keep stale
credentials for that account.

## `initialize`

Method:

- `initialize`

Required params:

- `clientInfo.name`
- `clientInfo.version`

Optional params:

- `capabilities.experimentalApi`
- `capabilities.optOutNotificationMethods`

`decodex` should declare itself explicitly as a non-interactive orchestration client.
- `dynamicTools` requires `capabilities.experimentalApi = true` during `initialize`.
- This experimental API enablement is part of the JSON-RPC handshake, not a `features.*` config flag in `~/.codex/config.toml`.
- The `initialize.codexHome` response must match Decodex's expected shared Codex
  home. A mismatch fails dispatch before `initialized`, account login,
  `thread/start`, or `thread/resume`, and the failure message must report the
  resolved and expected homes without including account tokens or other secrets.

## `thread/start`

Method:

- `thread/start`

The MVP thread start request owns these fields:

- `cwd`
- `dynamicTools` when the run exposes issue-scoped tracker tools
- `developerInstructions`
- `ephemeral` only for synthetic Decodex probe threads

For app-server 0.141 and newer, `dynamicTools` is a tagged union. Decodex keeps its
internal logical tool declaration flat so callback authorization can still match
`item/tool/call.namespace`, but the request wire shape is:

- unnamespaced tool: `{ "type": "function", "name": ..., "description": ..., "inputSchema": ... }`
- namespaced tools: `{ "type": "namespace", "name": ..., "description": ..., "tools": [...] }`,
  where each nested tool is a `type:function` entry with its own description and
  input schema.

Decodex must not inject project-owned config, model, personality, service-tier, sandbox, or approval-policy overrides into `thread/start`. Child runs inherit runtime defaults from the active Codex runtime.

`ThreadStartResponse` returns the effective thread plus the effective execution settings.
`thread/start` is allowed a longer bounded startup timeout than ordinary metadata
requests because current Codex app-server builds may need to hydrate tool, skill, and
thread runtime state before the first response. The longer timeout is still a startup
transport timeout, not a model-execution wait.

## `thread/resume`

Method:

- `thread/resume`

The resume request owns these fields:

- `threadId`
- `cwd`
- `developerInstructions`

Decodex must not inject project-owned config, model, personality, service-tier, sandbox, or approval-policy overrides into `thread/resume`. Resumed child runs keep inheriting runtime defaults from the active Codex runtime.

## `thread/goal/*`

Methods:

- `thread/goal/set`
- `thread/goal/get`
- `thread/goal/clear`

These methods are required for retained lane execution. Decodex calls them when a
phase-goal controller has a scoped goal for the run and treats missing methods as an
unsupported app-server capability failure.

`thread/goal/set` request fields:

- `threadId`
- `objective`
- `status`, set to `active` by Decodex
- `tokenBudget`, optional

`thread/goal/get` request fields:

- `threadId`

`thread/goal/clear` request fields:

- `threadId`

Goal responses should include the active `goal` object with `threadId`, `objective`,
`status`, optional `tokenBudget`, `tokensUsed`, `timeUsedSeconds`, and timestamps.
Decodex recognizes `active`, `paused`, `blocked`, `usageLimited`, `budgetLimited`,
and `complete` statuses. A missing goal after Decodex set one is an invalid
phase-goal state, not a successful no-op.

Decodex clears a thread goal best-effort only after the lane reaches an explicit
terminal completion path. Clear failures remain diagnostics and do not replace the
terminal tracker contract.

## `turn/start`

Method:

- `turn/start`

Required params:

- `threadId`
- `input`

The MVP turn start request owns these fields:

- `threadId`
- `input`

Decodex must not inject project-owned config, model, personality, service-tier, sandbox, or approval-policy overrides into `turn/start`.

`TurnStartResponse` returns the accepted turn object.

Within one bounded Decodex run attempt, the runtime may start multiple turns on the same thread. Thread-level settings remain stable from `thread/start`; continuation policy such as `execution.max_turns` and between-turn tracker revalidation stays in Decodex, not in the app-server protocol.

## `turn/interrupt`

Method:

- `turn/interrupt`

Decodex's intended use is soft active-turn interruption from a CLI/API operator
control. It should target the current known thread and turn, request a graceful turn
stop through app-server, and leave outcome classification to the Decodex runtime.

Request parameters:

```json
{
  "threadId": "<thread id>",
  "turnId": "<turn id>"
}
```

Decodex treats the app-server response as protocol evidence rather than a private
payload to expose. The local control response records a summary such as object keys or
array length, plus a normalized result:

- `soft_delivered` when app-server accepts the JSON-RPC request
- `soft_failed` when the method is unsupported, times out, or returns another protocol
  error
- `rejected` when the child process finds that the requested project, issue, run,
  attempt, thread, or turn no longer matches the active turn

`turn/interrupt` must not:

- mutate tracker state directly
- imply manual attention or review handoff by itself
- clear leases without runtime classification
- replace the hard-interrupt process fallback when app-server is unreachable

Decodex prefers `turn/interrupt` before signaling the child process. A hard interrupt
remains only a fallback after explicit operator intent and after soft interrupt is
unavailable, times out, or cannot be routed to the live app-server session.

## `turn/steer`

Method:

- `turn/steer`

Decodex's intended use is operator-supplied steer text for an active lane through the
CLI/API control surface. The bottom-layer app-server/protocol/runtime shape must not
hard-limit task content categories. It should carry the operator's instruction broadly
and leave policy constraints to Decodex audit, privacy, workflow, recovery, and
agent-skill layers.

`turn/steer` must not be treated as:

- a tracker mutation
- a hidden task replacement
- a bypass around review, validation, or terminal finalization
- a way for an agent to self-author new scope without an operator request

If a requested steer materially replaces the issue objective or acceptance contract,
Decodex must route that as explicit lifecycle/requeue work instead of silently steering
the active lane into a different task.

## `thread/inject_items`

Method:

- `thread/inject_items`

Raw item injection is deferred as an operator feature. Decodex does not expose
`thread/inject_items` through lane-control CLI/API in this rollout because raw item
insertion has broader transcript-shaping semantics than the intended operator steer
contract. Use `turn/steer` for active-lane steering.

## `command/exec`

Method:

- `command/exec`

Decision:

- Adopt `command/exec` only for lightweight app-server-side health checks and preflight commands that require an already-open app-server connection but do not require a Codex thread, turn, dynamic tools, tracker writes, repo-gate classification, or agent reasoning.
- The first supported Decodex use is `decodex probe`: after `initialize` and before `thread/start`, the probe runs a bounded standalone command through `command/exec` and verifies its buffered exit code, stdout, and stderr.

Required health-check request constraints:

- Use an argv vector in `command`.
- Set `cwd` to the target worktree or probe directory being checked.
- Set a short `timeoutMs`.
- Set `outputBytesCap`.
- Prefer buffered output for health checks; use streaming only if a future health check has a concrete need for stdin, PTY, resize, or early termination.

Do not use `command/exec` for:

- Decodex run execution. Agent work must still use `thread/start` or `thread/resume` plus `turn/start`.
- Repo-native gates from `WORKFLOW.md`. `canonicalize_commands` and `verify_commands` remain local repo-gate commands with their existing failure classes and tracked-file cleanliness check.
- Workspace lifecycle hooks. `execution.workspace_hooks` remain target-repository commands supervised by Decodex at worktree create/remove boundaries.
- The `_attempt` child process or other Decodex process supervision.
- Git, GitHub, tracker, or review handoff actions whose credentials, side effects, and failure classes are already modeled by Decodex-specific code.

Rationale:

- The generated protocol defines `command/exec` as a standalone argv command in the server sandbox without creating a thread or turn, with a final response after process exit.
- That shape is useful for cheap app-server environment checks such as "can the app-server execute a small command in this cwd?".
- That shape does not replace Decodex's run, repo-gate, credential, tracker, or process-supervision boundaries because it has no issue lease, thread/turn lifecycle, dynamic tool bridge, review handoff semantics, repo-gate failure model, or tracked-file cleanup contract.

## Notification handling

### `thread/started`

- Record the created thread identifier.

### `thread/status/changed`

- Track whether the thread is `active`, `idle`, `systemError`, or `notLoaded`.
- `waitingOnApproval` and `waitingOnUserInput` are policy violations for the MVP because Decodex runs are non-interactive and must inherit a Codex runtime policy that does not require manual approval or user input.

### `turn/started`

- Record the turn identifier and transition the local run into `running`.
- When this id differs from the id returned by `turn/start` but the thread id matches,
  adopt the notification id as the current active turn id before filtering later
  turn-scoped events.

### `turn/completed`

- Record the completed turn payload.
- Classify the turn as success, retryable failure, or terminal failure.
- A same-thread completion for the adopted notification turn id completes the active
  Decodex turn even when the original `turn/start` response id was different.
- A terminal non-`completed` turn status without a `turn.error` payload is still a
  structured app-server turn failure, not a generic runtime error. Decodex records
  `app_server_turn_missing_error_payload`, preserves the terminal status such as
  `interrupted` in the private error message, and routes retry or retry-budget
  exhaustion through the same retained-lane recovery path as other turn failures.

## Error handling

- JSON-RPC transport failure before a thread session is attached is a retryable
  startup failure. This covers the client waiting on `initialize`,
  `account/login/start`, `thread/start`, or `thread/resume`.
- `thread/start` and `thread/resume` may use a longer bounded timeout than ordinary
  metadata requests; they still fail as startup transport failures when the timeout is
  exhausted before a thread id is attached.
- If that startup transport failure exhausts the registered retry budget, the
  terminal failure must still preserve `app_server_transport_disconnected`.
- JSON-RPC transport failure after a thread session is attached, including
  `turn/start` or turn execution waits, is a human-required transport failure
  because blind retry can duplicate turn-side effects.
- Turn failure with `codexErrorInfo = "usageLimitExceeded"` is a retryable capacity
  failure until the registered retry budget is exhausted, so account-pool re-selection
  can recover without operator attention.
- `thread/status/changed` with `systemError` is a failed run.
- Turn completion with codex error information must be classified into:
  - retryable failure
  - terminal failure requiring human attention

The failure classifier must consider retry budget from the registered project `WORKFLOW.md` policy.

## Ownership boundaries

`decodex` owns:

- child process lifecycle
- request identifiers
- JSON-RPC framing
- local journaling of protocol messages
- mapping repo workflow policy into request fields other than inherited runtime execution policy
- servicing issue-scoped tracker tool calls
- run classification and fallback reconciliation writes when the agent never reached a tracker update

The downstream repository policy owns:

- repo-specific instructions
- repo-native gate commands
- issue eligibility parameters
- workflow state and label names
- when and why the coding agent should perform tracker writes during the run

## Probe contract

The `decodex probe` command must verify at least:

1. `codex app-server` is invocable locally.
2. Schema generation succeeds.
3. The schema contains `initialize`, `command/exec`, `thread/start`, and `turn/start`.
4. The schema contains `thread/status/changed` and `turn/completed`.
5. The local client can complete the bounded app-server capability preflight after
   `initialize` and before `thread/start`.
6. The local client can complete one bounded standalone `command/exec` health check after `initialize`.
7. The local client can complete one ephemeral
   `dynamicTools -> item/tool/call -> response` round trip and still finish with the
   expected final output without materializing a probe thread on disk.

The probe command is the first gate before deeper orchestrator logic depends on the protocol.

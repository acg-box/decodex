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

## Compatibility range

The current Decodex app-server support range is capability-gated rather than a broad
"latest Codex" promise. A Codex CLI or bundled Codex binary is inside the supported
range only when all of these are true:

- `codex app-server generate-json-schema --experimental` succeeds.
- The generated schema contains the Decodex-owned request and notification contract in
  this spec, including `initialize`, `thread/start`, `thread/resume`, `turn/start`,
  `thread/archive`, `command/exec`, the bounded preflight methods, `item/tool/call`,
  dynamic tool `namespace`, dynamic tool `deferLoading`, `inputText` tool responses,
  and `PluginListParams.marketplaceKinds`.
- `decodex probe stdio://` completes the app-server capability preflight,
  standalone `command/exec` health check, and dynamic-tool round trip with
  `PROBE_OK`.

As of the 2026-06-02 self-compatibility pass, the verified local range is:

| Codex surface | Version | Evidence |
| --- | --- | --- |
| `PATH` `codex` | `codex-cli 0.136.0` | Generated `--experimental` schema contains the required methods and fields; `decodex probe stdio://` returned `PROBE_OK`. |
| Codex Beta app bundled `codex` | `codex-cli 0.136.0-alpha.2` | Running `decodex probe stdio://` with the bundle resource directory first on `PATH` returned `PROBE_OK`. |

The same pass compared that range against upstream Codex:

- GitHub release `rust-v0.136.0` is covered by the verified `PATH` `codex-cli 0.136.0`
  probe above.
- Upstream `main` commits after `rust-v0.136.0` are outside the local support claim
  until Radar review, schema regeneration, and `decodex probe stdio://` cover that
  newer head or release.
- The checked-in upstream review queue generated on 2026-06-02 contained 40 queued
  `openai/codex` subjects, including critical and high-priority app-server protocol,
  plugin/tool metadata, sandbox/config, and release-packaging candidates.
  Those queue entries are compatibility watch items, not adoption authorization.

The previous 2026-05 local refresh covered `codex-cli 0.132.0-alpha.1` from `PATH`
and the Codex Beta app bundle's `codex-cli 0.131.0-alpha.9`. Treat those as historical
compatibility evidence, not the current upgrade target.

Current upstream Codex signals are beyond the local support claim whenever they are
newer than the latest locally probed version, or when checked-in Radar queue entries
flag app-server protocol, plugin metadata, dynamic tool, sandbox/config, GitHub/Linear
routing, or retained-lane lifecycle risk that has not yet been source-reviewed and
probed locally. In that case Decodex must not force an upgrade. It should keep running
the latest locally verified Codex surface, route the upstream change through Radar
review, regenerate the app-server schema, run `decodex probe stdio://`, and only then
promote the new Codex version or protocol shape into this compatibility range.

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
  - `turn/start`
  - `thread/archive` after successful completion writeback, for every locally
    recorded terminal attempt thread on the issue that has not already recorded a
    successful archive event
- Required notifications for the MVP:
  - `thread/started`
  - `thread/status/changed`
  - `turn/started`
  - `turn/completed`

Additional notifications may be recorded opportunistically for diagnostics.

The follow-up alignment phase should also record tool-related requests and notifications needed for issue-scoped tracker writes.

Decodex records a compact local protocol summary from high-value structured
notifications instead of scraping transcripts. The summary may include
`turn/started`, `turn/completed`, plan updates, diff updates, item
start/completion, command output deltas, server request responses, account updates,
rate-limit updates, warning/deprecation notices, model reroutes/verifications, and
thread token-usage updates. This summary is published through the operator status
snapshot and dashboard only; high-frequency protocol details remain out of Linear
unless an existing lifecycle event summarizes them.

## Required request flow

1. Start the child process.
2. Send `initialize`.
3. Run the bounded capability preflight with `config/read`, `model/list`,
   `modelProvider/capabilities/read`, `skills/list`, `plugin/list`, and
   `mcpServerStatus/list`.
4. When `[codex.accounts]` is enabled, select a shared ChatGPT account and send
   `account/login/start` with `chatgptAuthTokens`.
5. Send `thread/start`.
6. Send `turn/start`.
7. Consume notifications until that turn reaches a terminal outcome.
8. If the project-owned continuation policy allows another same-thread turn, send another `turn/start` on the same thread.
9. Persist the local run journal and classify the bounded run result.
10. After successful completion writeback, best-effort archive all locally recorded
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
one app-server output timeout before failing the lane. If the retry is exhausted,
the terminal failure must remain an app-server preflight failure, report
`app_server_plugin_list_timeout`, and include the `plugin/list` timeout cause in local
preflight evidence and operator recovery output rather than looking like a repository
implementation failure.

When dynamic tools are enabled, `decodex` must also:

1. Register the tool surface in `thread/start.dynamicTools`.
2. Answer `item/tool/call` requests with `DynamicToolCallResponse`.
3. Serialize dynamic tool output items with schema-approved `type` values such as `inputText`.
4. Keep every `dynamicTools[].name` and populated `dynamicTools[].namespace` within the app-server identifier pattern `^[a-zA-Z0-9_-]+$`.
5. Validate incoming `item/tool/call` thread, turn, tool-name, namespace, and response shape before treating the request as handled.

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

Decodex must not inject project-owned config, model, personality, service-tier, sandbox, or approval-policy overrides into `thread/start`. Child runs inherit runtime defaults from the active Codex runtime.

`ThreadStartResponse` returns the effective thread plus the effective execution settings.

## `thread/resume`

Method:

- `thread/resume`

The resume request owns these fields:

- `threadId`
- `cwd`
- `developerInstructions`

Decodex must not inject project-owned config, model, personality, service-tier, sandbox, or approval-policy overrides into `thread/resume`. Resumed child runs keep inheriting runtime defaults from the active Codex runtime.

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

### `turn/completed`

- Record the completed turn payload.
- Classify the turn as success, retryable failure, or terminal failure.

## Error handling

- JSON-RPC transport failure before `thread/start` succeeds is a retryable startup failure.
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

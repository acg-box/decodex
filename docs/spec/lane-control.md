# Lane-Control Specification

Purpose: Define Decodex operator lane-control capabilities and the boundary between
bottom-layer protocol support and higher-level policy guardrails.
Status: normative
Read this when: You are implementing, validating, or using CLI/API controls for active
or retained Decodex lanes.
Not this document: The full runtime state machine, the low-level app-server method
schema, dashboard layout, or tracker-tool payload schema.
Defines: The lane-control capability matrix, supported and deferred controls, audit
requirements, and policy boundary for inspect, pause/resume, scan, interrupt, steer,
retained retry/resume, and manual-attention controls.

## Scope

Lane control is the operator-facing ability to inspect and influence a Decodex-owned
lane without bypassing the runtime lease, tracker, retained-worktree, and review
contracts.

The first supported operator-control surface for this rollout is CLI/API. Active-lane
UI controls are intentionally deferred. The dashboard may show local runtime state for
observation, but it must not become the primary place where agents or operators author
steer, retry, task replacement, or lifecycle mutations before the CLI/API contract is
implemented and audited.

Bottom-layer steer support must not hard-limit task content. The app-server,
protocol, and runtime layer should expose steer broadly enough to pass operator-supplied
instructions through to a live lane. Constraints belong above that layer: project
policy, audit records, recovery rules, workflow contracts, privacy guards, and
agent-facing skills must guide responsible use.

## Capability Matrix

| Capability | Contract status | Current implementation evidence | Required behavior |
| --- | --- | --- | --- |
| Inspect lane state | Supported | `decodex lane inspect <ISSUE>`, `decodex status`, `decodex status --json`, `decodex diagnose --json`, `decodex evidence <ISSUE>`, `GET /api/lane/inspect`, operator snapshots, and dashboard views | Always inspect before mutating or steering. Inspection must not mutate tracker state, runtime DB rows, worktrees, or app-server turns. |
| Project dispatch pause | Supported for future dispatch | `decodex project disable <service-id>` and the runtime project enabled flag | Pause prevents new dispatch for the project. It must not kill or rewrite already active lanes. |
| Project dispatch resume | Supported for future dispatch | `decodex project enable <service-id>` and the runtime project enabled flag | Resume re-enables future dispatch after the operator has inspected blockers, capacity, and queue state. |
| Linear scan request | Supported | `POST /api/linear-scan` with optional `projectId` | Queue a scan for the next control-plane tick while respecting tracker backoff. This is an intake/status refresh request, not an execution command. |
| Run-control channel foundation | Supported foundation | Active attempts publish a local `.decodex-run-control/*` channel record, runtime SQLite `run_control_channels`, operator status `control_capability`, and private `control_action` audit events | Route lane-control mutations through the active attempt's project, issue, run id, attempt, thread id, current turn id, active lease, and local channel metadata. Invalid or stale requests fail closed and remain local audit evidence. |
| Soft interrupt | Supported CLI/API control | `decodex lane interrupt <ISSUE> --run-id <RUN_ID>` and `POST /api/lane/interrupt` write a run-control request that the active app-server child delivers with `turn/interrupt` | Prefer soft interrupt before hard interruption when the active turn id is known and the app-server capability is present. Soft interrupt requests a graceful turn stop and records the protocol outcome when app-server returns one. |
| Hard interrupt fallback | Explicit fallback only | `decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force` and `POST /api/lane/interrupt` with `"force": true` classify process signaling as `hard_interrupt_fallback` | Use only when soft interrupt is unavailable, timed out, or impossible because the process or app-server boundary cannot be reached. Preserve retained worktree evidence and runtime classification. |
| Steer active lane | Supported CLI/API control; bottom-layer method stays broad | `decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>`, canonical `POST /api/lane/steer`, legacy alias `POST /api/lane-steer`, local run-control steer request/response files, app-server `turn/steer`, private `control_action` audit events, and protocol activity `turn/steer` summaries | Pass operator-supplied steer text through CLI/API to the current active turn. Require `expectedTurnId`; stale turn ids fail closed. Do not narrow the protocol to a fixed set of task-content categories. Apply policy, audit, privacy, and lifecycle guardrails above the protocol. |
| Retained resume/retry | Supported through runtime lifecycle | `decodex run <ISSUE>`, retry scheduling, retained worktree recovery, and `thread/resume` for same-thread app-server continuation | Resume only when retained worktree, issue, branch, PR, and runtime evidence still prove the same lane. Treat ambiguous lineage as manual attention. |
| Manual attention | Supported terminal control path | `decodex:needs-attention`, `issue_comment(kind = "manual_attention")`, and `issue_terminal_finalize(path = "manual_attention")` | Stop automation when policy requires a human decision. Explain the blocker through structured public fields and keep private evidence local. |
| Task replacement | Deferred lifecycle work | No supported active-lane replacement command | Do not use steer or raw injection to replace the task silently. Treat replacement as explicit lifecycle work: pause/stop if needed, update or requeue the issue, or create a new issue/lane. |
| Raw thread item injection | Unsupported as an operator feature | No Decodex operator path for `thread/inject_items` | Do not expose raw `thread/inject_items` to operators in this rollout. Use `turn/steer` for operator steer. |
| Active-lane UI authoring controls | Deferred | Existing dashboard views and low-level handlers are not the CLI/API-first lane-control contract | Do not add dashboard steer, retry, or task-replacement controls in this rollout. Ship CLI/API first, then promote UI controls only after audit and policy behavior is settled. |

## Inspect-First Rule

Before any lane-control mutation, the operator or agent must inspect:

- project id and registered project enablement
- issue identifier and tracker state
- branch and retained worktree ownership
- run id, attempt number, thread id, and current turn id when available
- active lease state, process liveness, and protocol activity
- recent private evidence and any public Linear lifecycle signal
- PR lineage when the lane already crossed into review handoff

If inspection cannot prove the requested lane identity, do not steer, interrupt, retry,
or resume. Use the manual-attention path or a read-only recovery diagnosis instead of
guessing.

## Run-Control Channel Foundation

Every live app-server attempt publishes a per-attempt local control capability when
Decodex still owns the active lease for the run. The current mechanism is a local file
channel under the run worktree's `.decodex-run-control/` directory plus a
`run_control_channels` runtime SQLite row. This is foundation plumbing only: it proves
where an active attempt can receive future control traffic without exposing steer,
interrupt, or task-replacement semantics by itself.

The channel row is scoped by project id, issue id, run id, attempt number, transport,
channel path, channel status, and publish/update timestamps. The current thread id and
turn id remain on the run attempt row and are projected together with the channel as
operator `control_capability` metadata. `decodex status`, JSON operator snapshots, and
private evidence readback may show this local capability, but Linear must not receive
host-local channel paths or raw control payloads.

A control request is valid only when all of the following hold:

- the requested run exists
- requested project id, issue id, run id, and attempt number match the active attempt
- requested thread id and turn id, when supplied, match the current attempt values
- the active lease for the issue is held by the same project and run id
- the attempt status is active
- the persisted channel row is active and the local channel path still exists

Any mismatch fails closed. Rejections are not converted into process signals, tracker
state changes, or worktree mutations.

## Soft And Hard Interrupts

Soft interrupt is the preferred active-turn stop path. A compliant soft interrupt:

- targets the current known app-server turn
- requests `turn/interrupt` instead of signaling the process
- records an audit event with project id, issue id, run id, attempt, thread id, turn id,
  operator reason, and outcome
- leaves tracker state, retry policy, and retained-worktree classification to the
  Decodex runtime

The supported operator commands are:

- `decodex lane inspect <ISSUE> [--run-id <RUN_ID>] [--json]`
- `decodex lane interrupt <ISSUE> --run-id <RUN_ID> [--json] [--reason <TEXT>]`
- `decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force [--json] [--reason <TEXT>]`

The local HTTP API mirrors those semantics:

- `GET /api/lane/inspect?projectId=<service-id>&issue=<ISSUE>[&runId=<RUN_ID>]`
- `POST /api/lane/interrupt` with JSON fields `projectId`, `issue`, `runId`,
  optional `reason`, and optional `force`

When only one project is enabled on the local listener, the HTTP API can infer
`projectId`; otherwise the request must include it. CLI/API responses include the
lane identity, app-server thread and turn ids when known, process liveness summary,
soft-interrupt classification, hard-fallback classification when used, and next action.
They do not include private payload bodies.

Hard interrupt is a fallback, not the normal operator control. A hard interrupt may
signal the recorded child process only after explicit `--force` or `"force": true`
operator intent. The fallback emits `hard_interrupt_fallback`, preserves local
evidence, marks an active attempt as `interrupted` when a recorded child is signaled,
clears or retains ownership according to the runtime contract, and avoids pretending
the agent completed a terminal path.

## Steer

Steer is an active-lane instruction from the operator to the running Codex turn. The
bottom-layer protocol shape must not decide which task topics are acceptable. It should
carry the operator's steer text broadly, subject only to generic transport and schema
requirements.

The supported operator commands are:

- `decodex lane inspect <ISSUE> [--run-id <RUN_ID>] [--json]`
- `decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT> [--json]`

The local HTTP API mirrors those semantics:

- `POST /api/lane/steer` with JSON fields `projectId`, `issue` or `issueId`,
  `runId`, `expectedTurnId`, `message`, and optional `waitTimeoutMs`
- `POST /api/lane-steer` remains a compatibility alias for the same request

The `expectedTurnId` precondition is mandatory. If the current active turn no longer
matches the supplied turn id, the request fails closed with `stale_expected_turn_id`
and remains local audit evidence instead of being delivered to app-server.

Higher layers own guardrails:

- CLI/API must require explicit operator-supplied steer text and a target lane identity.
- Decodex must audit steer requests in local runtime evidence before or alongside
  delivery to app-server.
- Privacy and public-text guards decide what, if anything, may be mirrored to Linear.
- Agent skills must tell agents to use steer only when the operator supplies it, and to
  inspect the lane first.
- Workflow policy decides whether the steered lane may continue, must retry, or must
  stop for human attention after the turn resolves.

Steer is not task replacement. If the operator wants a different issue, a materially
different goal, or a new acceptance contract, the correct path is lifecycle/requeue
work, not a hidden active-lane content swap.

## Retained Resume And Retry

Retained resume and retry are lifecycle controls, not prompt injection controls.
Decodex may re-enter a retained lane only when the runtime can prove that the issue,
branch, worktree, run evidence, and PR lineage still match the owned lane.

Supported retained controls include:

- normal retry scheduling after retryable failures with remaining budget
- same-thread `thread/resume` when Decodex has a valid thread id and the app-server
  accepts it
- explicit one-issue automation with `decodex run <ISSUE>` when the registered
  workflow and current lane state make that issue eligible
- recovery diagnosis and explicit rebind paths defined by the retained-lane specs

Unsupported retained controls include guessing a worktree from a branch name, clearing
runtime DB rows by hand, or changing tracker state to force dispatch when ownership
signals disagree.

## Manual Attention

Manual attention is the required stop when the operator or runtime cannot safely derive
the next lane action from authoritative signals. It is also the correct route when a
requested control would overwrite useful partial work, hide a blocker, or require
guessing human intent.

Agents must not simulate manual attention by editing tracker state directly. The valid
agent path is:

1. add the configured `decodex:needs-attention` label
2. call `issue_comment` with `kind = "manual_attention"` and structured public fields
3. call `issue_terminal_finalize(path = "manual_attention")`

Operators may later clear the blocker, clear the label, and requeue the lane through
the configured lifecycle.

## Audit And Privacy

Every supported control mutation should create local runtime evidence. At minimum, a
control audit record should identify the project, issue, run id, attempt, branch,
operator command source, requested capability, normalized result, and next action.

The run-control foundation records local `control_action` private execution events for
accepted, rejected, completed, failed, timed out, and fallback outcomes. These records
are scoped by the same project, issue, run id, and attempt tuple as other private
execution evidence. They are available through `decodex evidence <ISSUE> --run-id
<RUN_ID> --attempt <N>` and survive independently of any public Linear projection.

Linear public text remains sparse. Do not write steer text, raw command output, process
diagnostics, private evidence payloads, account details, or host-local paths into
Linear unless a schema-controlled public projection explicitly allows it.

## Implementation Status For This Rollout

Current code supports lane inspect, CLI project enable/disable, Linear scan requests,
soft interrupt, explicit hard-interrupt fallback, active-lane steer, retained
resume/retry lifecycle paths, and manual-attention finalization. Current code does not
expose dashboard lane-mutation controls and does not expose raw `thread/inject_items`
as an operator feature.

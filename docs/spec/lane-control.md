---
type: "Spec"
title: "Lane-Control Specification"
description: "Define Decodex operator lane-control capabilities and the boundary between bottom-layer protocol support and higher-level policy guardrails. Status: normative Read this when: You are implementing, validating, or using CLI/API controls for active or retained Decodex lanes. Not this document: The full runtime state machine, the low-level app-server method schema, dashboard layout, tracker-tool payload schema, or the step-by-step recovery sequence after a control action. Use [`../runbook/lane-control-recovery.md`](../runbook/lane-control-recovery.md) for post-control recovery decisions. Defines: The lane-control capability matrix, supported and deferred controls, audit requirements, and policy boundary for inspect, pause/resume, scan, interrupt, steer, retained retry/resume, and manual-attention controls."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/recovery.rs, apps/decodex/src/orchestrator/status.rs]
drift_watch: [decodex recover ghost-lane, decodex recover stale-active, stale_active_release, run_stale_active_recovery, linear_active_label_present, ghost_lane_cleanup_audit_present, mcp_test_fixture_ghost_lane, runtime_recovery_required, runtime_recovery_blocked, run_control_channels]
last_verified: 2026-06-30
---
# Lane-Control Specification

Purpose: Define Decodex operator lane-control capabilities and the boundary between
bottom-layer protocol support and higher-level policy guardrails.
Status: normative
Read this when: You are implementing, validating, or using CLI/API controls for active
or retained Decodex lanes.
Not this document: The full runtime state machine, the low-level app-server method
schema, dashboard layout, tracker-tool payload schema, or the step-by-step recovery
sequence after a control action. Use
[`../runbook/lane-control-recovery.md`](../runbook/lane-control-recovery.md) for
post-control recovery decisions.
Defines: The lane-control capability matrix, supported and deferred controls, audit
requirements, and policy boundary for inspect, pause/resume, scan, interrupt, steer,
retained retry/resume, and manual-attention controls.

## Scope

Lane control is the operator-facing ability to inspect and influence a Decodex-owned
lane without bypassing the runtime lease, tracker, retained-worktree, and review
contracts.

[`loop-runtime.md`](./loop-runtime.md) owns the natural-language-first research,
promotion, and internal Execution Program contract. Lane control does not expose that
program as a user-visible DAG surface. Inspect, steer, interrupt, retained retry, and
manual attention remain lane controls for already-owned runtime lanes.

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
| Project dispatch resume | Supported for future dispatch | `decodex project enable <service-id>` and the runtime project enabled flag | Resume re-enables future dispatch after the operator has inspected blockers, active work, and queue state. |
| Linear scan request | Supported | `POST /api/linear-scan` with optional `projectId` | Queue a scan for the next control-plane tick while respecting tracker backoff. This is an intake/status refresh request, not an execution command. |
| Run-control channel foundation | Supported foundation | Active attempts publish a local `.decodex-run-control/*` channel record, runtime SQLite `run_control_channels`, operator status `control_capability`, and private `control_action` audit events | Route lane-control mutations through the active attempt's project, issue, run id, attempt, thread id, current turn id, run lease, and local channel metadata. Invalid or stale requests fail closed and remain local audit evidence. |
| Soft interrupt | Supported CLI/API control | `decodex lane interrupt <ISSUE> --run-id <RUN_ID>` and `POST /api/lane/interrupt` write a run-control request that the active app-server child delivers with `turn/interrupt` | Prefer soft interrupt before hard interruption when the active turn id is known and the app-server capability is present. Soft interrupt requests a graceful turn stop and records the protocol outcome when app-server returns one. If the run-control resolver rejects soft delivery with `run_lease_missing`, preserve the observed process, channel, branch, and retained worktree evidence instead of hiding the lane as inactive. |
| Hard interrupt fallback | Explicit fallback only | `decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force` and `POST /api/lane/interrupt` with `"force": true` classify process signaling as `hard_interrupt_fallback` | Use only when soft interrupt is unavailable, timed out, or impossible because the process, app-server boundary, or run lease cannot be reached. A forced interrupt may signal the recorded child process after `run_lease_missing` only when inspection still identifies the same issue, run id, attempt, channel, and live process. Preserve retained worktree evidence and runtime classification. |
| Steer active lane | Supported CLI/API control; bottom-layer method stays broad | `decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>`, canonical `POST /api/lane/steer`, legacy alias `POST /api/lane-steer`, local run-control steer request/response files, app-server `turn/steer`, private `control_action` audit events, and protocol activity `turn/steer` summaries | Pass operator-supplied steer text through CLI/API to the current active turn. Require `expectedTurnId`; stale turn ids fail closed. Do not narrow the protocol to a fixed set of task-content categories. Apply policy, audit, privacy, and lifecycle guardrails above the protocol. |
| Retained resume/retry | Supported through runtime lifecycle | `decodex run <ISSUE>`, retry scheduling, retained worktree recovery, and `thread/resume` for same-thread app-server continuation | Resume only when retained worktree, issue, branch, PR, and runtime evidence still prove the same lane. Treat ambiguous lineage as manual attention. |
| Missing-issue ghost-lane recovery | Supported explicit recovery | `decodex status --live` and fresh daemon-cached `decodex status` project `ownership_state = ghost_lane`, `policy_state = runtime_recovery_required`, and `lane_control_next_action = run_ghost_lane_recovery`; `decodex recover ghost-lane diagnose [ISSUE]` and `decodex recover ghost-lane cleanup <ISSUE>` perform the read-only and mutating recovery paths | Only terminalize and clear a local run lease when tracker issue lookup proves the issue is missing and local inspection proves no worktree, live process, PR lineage, or review lifecycle record. Ordinary control-channel, thread/protocol, or private evidence still fails closed. The only exception is the narrow `mcp_test_fixture_ghost_lane` diagnosis for the historical PubFi MCP fixture: exact `PUB-012` / `run-12` attempt 1 with optional `thread-12` / `turn-12`, a missing control-channel file, and private evidence made only of `source = mcp-test` lane-control request events, fixture-matching `control_action` audit rows whose `source` is `mcp-test` or `cli`, and a prior validated `ghost_lane_cleanup` audit. A cleanup-audited missing-issue ghost with no retained worktree, live process, PR lineage, or review lifecycle must not remain a current or retained-attention lane. Any mixed private evidence, retained worktree, tracker issue, control-channel file, child-agent activity, live process, PR lineage, or review lifecycle record must fail closed and preserve attention. |
| Tracker-present stale active recovery | Supported explicit recovery | `decodex recover stale-active diagnose [ISSUE]`, `decodex recover stale-active release <ISSUE>`, queue status `reason = linear_active_label_present`, and `attention_next_action = run_stale_active_recovery` | Release only the service active label when tracker readback proves the issue exists and still carries `decodex:active:<service-id>`, while local inspection proves no needs-attention label, no live process, no source-progress worktree state, no uninspectable worktree state, no unmerged retained branch commits, no unavailable retained default-branch proof, no private source/review progress evidence, and no PR/review lineage or review-policy checkpoint. A run lease or active shared claim remains a blocker unless process identity proves the recorded child is gone, the run-activity marker run id and attempt match the latest leased run, the local lease belongs to that same project/run, and no external or incompatible shared claim is present. Dead-process runtime telemetry such as stale thread status, active local control-channel files, protocol events, child/protocol summaries, failed control attempts, implementation phase-goal rows, app-server no-diff loop guardrail checkpoints, no-progress harness outcomes, and probing checkpoints is recoverable only after process identity proves the recorded child is gone and the worktree/lineage/progress guards are clean. Local runtime evidence is checked under both the tracker issue id and visible issue identifier. Preserve the queue label if present. On non-dry-run, re-read tracker/runtime safety evidence without persistent cleanup before mutation, preflight local cleanup without mutation, clear only matching proven-dead local run leases, verify that no run lease or active shared claim has reappeared, terminalize the stale run attempt as `terminal_guarded`, retire the inactive control channel, clean only clean or marker-only retained worktree mappings, write a local private `stale_active_release` audit when a run attempt exists, repeat the run-lease/shared-claim guard, recheck tracker labels, and remove only the service active label as the final mutation. |
| Manual attention | Supported terminal control path | `issue_label_add` intent for `decodex:needs-attention`, `issue_comment(kind = "manual_attention")`, and `issue_terminal_finalize(path = "manual_attention")` | Stop automation when policy requires a human decision. Explain the blocker through structured public fields and keep private evidence local. |
| Task replacement | Deferred lifecycle work | No supported active-lane replacement command | Do not use steer or raw injection to replace the task silently. Treat replacement as explicit lifecycle work: pause/stop if needed, update or requeue the issue, or create a new issue/lane. |
| Raw thread item injection | Unsupported as an operator feature | No Decodex operator path for `thread/inject_items` | Do not expose raw `thread/inject_items` to operators in this rollout. Use `turn/steer` for operator steer. |
| Active-lane UI authoring controls | Deferred | Existing dashboard views and low-level handlers are not the CLI/API-first lane-control contract | Do not add dashboard steer, retry, or task-replacement controls in this rollout. Ship CLI/API first, then promote UI controls only after audit and policy behavior is settled. |

## Inspect-First Rule

Before any lane-control mutation, the operator or agent must inspect:

- project id and registered project enablement
- issue identifier and tracker state
- branch and retained worktree ownership
- run id, attempt number, thread id, and current turn id when available
- run lease state, process liveness, and protocol activity
- recent private evidence and any public Linear lifecycle signal
- PR lineage when the lane already crossed into review handoff

If inspection cannot prove the requested lane identity, do not steer, interrupt, retry,
or resume. Use the manual-attention path or a read-only recovery diagnosis instead of
guessing.

## Run-Control Channel Foundation

Every live app-server attempt publishes a per-attempt local control capability when
Decodex still owns the run lease for the run. The current mechanism is a local file
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

When a worktree activity marker for the same run id and attempt number carries a
thread id or turn id missing from the run attempt row, Decodex may hydrate the missing
attempt identity before resolving local control. Marker identity must not override an
already recorded attempt thread id or turn id. Inspect/status/control resolution must
therefore compare against one canonical current-attempt identity instead of letting UI
readback and control matching use different sources.

A control request is valid only when all of the following hold:

- the requested run exists
- requested project id, issue id, run id, and attempt number match the active attempt
- requested thread id and turn id, when supplied, match the current attempt values
- the run lease for the issue is held by the same project and run id
- the attempt status is active
- the persisted channel row is active and the local channel path still exists

Any mismatch fails closed. Rejections are not converted into process signals, tracker
state changes, or worktree mutations.

`run_lease_missing` is a soft-control rejection, not proof that execution is gone.
The audit payload must retain the requested project, issue, run id, attempt, current
thread and turn ids, active channel metadata when present, branch, retained worktree
mapping, and operator-local process/protocol liveness context. If the operator used
`--force` or `"force": true`, the interrupt command may then take the explicit hard
fallback path against the recorded child process without rebinding the queue lease or
pretending that soft app-server control succeeded.

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

When forced interrupt follows an `run_lease_missing` soft-control rejection, the
CLI/API response must show both facts: soft control was rejected, and hard fallback was
attempted only because force was explicit and process evidence was still present. If no
signalable child process is recorded, the hard-fallback report must say it was
unavailable or found no process and direct the operator to inspect retained evidence;
it must not imply a graceful interrupt or successful recovery.

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

## Missing-Issue Ghost Lanes

A missing-issue ghost lane is a local runtime lease that still appears in current
live or fresh daemon-cached status but no longer has a tracker issue entity,
retained worktree, ordinary control-channel evidence, ordinary private evidence,
live process/thread/protocol evidence, or PR/review lineage. Tracker-backed status
must not point this class at review-checkpoint recording or review-handoff rebind,
because those paths require a tracker issue or retained PR lane. The no-cache
local-runtime status fallback is intentionally not tracker-backed and does not prove
issue absence by itself.

The `mcp_test_fixture_ghost_lane` recovery classification is narrower than ordinary
ghost-lane recovery. It exists only to clear the historical PubFi MCP fixture lane
whose private evidence is entirely lane-control audit records: request rows with
`source = mcp-test` and `control_action` rows with either `source = mcp-test` or a
fixture-matching `source = cli` request for `pubfi` / `PUB-012` / `run-12` attempt 1.
That classification may tolerate the fixture's stale control-channel row,
thread/turn references, protocol event count, and protocol activity summary when the
control-channel file, tracker issue, worktree, PR lineage, review lifecycle, child
activity, and live process are absent. Any other private event or runtime progress
evidence returns the lane to `runtime_recovery_blocked`.

`decodex recover ghost-lane diagnose [ISSUE]` is read-only. It reports public-safe
condition names such as `tracker_issue_missing`, `worktree_missing`,
`control_channel_missing`, `private_evidence_missing`, and
`review_lineage_missing`. `cleanup` is the explicit mutating path and should be run
with `--dry-run` first. Cleanup writes local private audit evidence, marks the run
attempt `terminal_guarded`, removes only an already-missing worktree mapping, and
clears the local lease. It must not mutate Linear when the tracker issue is missing.
If a prior cleanup already wrote a validated `ghost_lane_cleanup` audit with
`cleared_run_lease = true`, empty blockers, and evidence for missing tracker issue,
missing worktree, and missing review lineage, status and diagnose treat that audit as
idempotent recovery evidence instead of ordinary private evidence. Such a row is
history-only unless a retained worktree, live execution signal, PR lineage, review
lifecycle, or mixed private evidence reintroduces a fail-closed blocker.
Tracker-backed retained-lane scans must isolate stale local issue identifiers during
refresh. A missing or locally shaped issue id can be dropped only after refresh proves
it is not a valid tracker issue; it must not abort status or dry-run candidate
selection for unrelated registered project issues.

If inspection finds a tracker issue, retained worktree, live execution signal,
ordinary control-channel row or file, ordinary private evidence, review lifecycle row,
or PR lineage, tracker-backed status uses `policy_state = runtime_recovery_blocked`
and the recovery command refuses to clean the lane. Operators then inspect the blocker
instead of deleting runtime rows.

## Tracker-Present Stale Active Ownership

A tracker-present stale active lane is different from a missing-issue ghost lane. The
tracker issue still exists and carries `decodex:active:<service-id>`, but local
runtime ownership is no longer live: no run lease, no active shared claim, no
signalable process, no source-progress worktree state, no unmerged retained branch
commits, no unavailable retained default-branch proof, and no private source/review,
PR/review, dirty-worktree, or uninspectable-worktree evidence. Local runtime evidence is read
under both the tracker issue id and the visible issue identifier so stale identifier
rows cannot be hidden by tracker id canonicalization. A retained `thread_id` or
`turn_id` alone is stale metadata, not proof of live work. Stale thread status,
active local control-channel files, protocol events, child/protocol summaries, failed
control attempts, implementation phase-goal rows, app-server no-diff loop guardrail
checkpoints, no-progress harness outcomes, and probing checkpoints are recoverable telemetry
only when process identity proves the recorded child is gone and worktree, branch,
private progress, and lineage checks are all clean. If the issue is also queued,
ordinary dispatch must remain
blocked with `linear_active_label_present` until an explicit recovery command releases
that stale active ownership.

`decodex recover stale-active diagnose [ISSUE]` is read-only. With an issue selector
it inspects that tracker issue; without a selector it lists service active-label
issues so active-only stale ownership remains discoverable even when the issue is not
currently visible in queue status. `release <ISSUE>` is the only mutating path and
should be run with `--dry-run` first. Diagnose and dry-run use non-mutating shared
claim observation: an unlocked stale claim anchor may be observed as inactive, but it
is not deleted by the read-only path. Release re-reads tracker/runtime safety evidence
before any mutation, then preflights local cleanup without mutation; if that succeeds
it preserves the queue label, clears only a matching proven-dead local run lease,
verifies that no run lease or active shared claim has reappeared, marks the stale
attempt `terminal_guarded`, retires the inactive
run-control channel, removes only clean or marker-only retained worktree mappings,
writes local private `stale_active_release` audit evidence when a matching stale run
exists, repeats the run-lease/shared-claim guard, rechecks tracker labels, restores a
queued issue from the configured in-progress state to the first configured startable
state when the queue label is preserved, and removes only the service active label.
The `terminal_guarded` write applies to stale active attempts that are still active
and to terminal-looking app-server failures such as `failed` or `interrupted`; the
guard records that recovery, not the old child/protocol telemetry, now owns the final
label-release safety check.

If a stale-active release attempt stops after local cleanup but before the final
tracker-label mutation, reentry is allowed only when local evidence proves the same
run attempt is already `terminal_guarded` or still carries a terminal-looking
app-server status such as `failed` or `interrupted`, the control channel is inactive
or was never published, retained worktree mapping/path cleanup completed,
`stale_active_release` audit evidence exists, and the remaining blockers are limited
to stale protocol/activity summaries from the old run. Reentry still repeats the
run-lease/shared-claim, review-lineage, and tracker-label guards before removing the
service active label.
If that final active-label mutation already happened but the queued issue remained in
the configured in-progress state, reentry is allowed only with the same run/attempt
release audit and completed local cleanup evidence; it may restore the issue to the
first configured startable state without hand-editing tracker state.

Stale active recovery must fail closed when any blocker indicates possible live or
useful work: run lease or active shared claim that is not bound to the latest
proven-dead local run/attempt, `decodex:needs-attention`, live process,
unknown process liveness for a runtime marker, tracked or untracked non-runtime
worktree changes, failed worktree status inspection, unavailable retained
default-branch proof, retained branch commits not reachable from the default branch,
private source/review progress evidence, review lifecycle, review-policy checkpoint,
or PR/review lineage under either the tracker issue id or issue identifier. Runtime
telemetry from a proven-dead child is allowed
recovery evidence because it describes stale execution, not source progress; failed
lane control attempts such as `control_action` rows with
`reason = run_lease_missing` are allowed because they prove supported controls were
tried and made no source progress.

## Manual Attention

Manual attention is the required stop when the operator or runtime cannot safely derive
the next lane action from authoritative signals. It is also the correct route when a
requested control would overwrite useful partial work, hide a blocker, or require
guessing human intent.

Retained partial progress is not a manual-attention stop while another runtime owner
is still authoritative for the same run. If the current run activity marker records a
retry schedule, the retry scheduler owns the next action. If the marker records a live
`repo_gate` operation, that gate remains the active owner until the process exits or a
later marker changes ownership. If stalled retained work still has an active phase
goal and the latest progress evidence has no blockers or decision request, Decodex
must try phase-goal recovery and schedule the next continuation instead of writing
`partial_progress_retained`.

Loop guardrail outcomes are not active-lane controls. They first stop the current
ineffective strategy. Engineering convergence reasons such as `validation_repeat`,
`no_effective_diff`, `remaining_delta_unchanged`, or `review_churn` may then enter
autonomous architecture recovery only after an Architecture Recovery Packet and
Authority Boundary Check prove the next strategy is inside the Authority Envelope and
recovery budget remains. Boundary, dependency, uncovered-direction, ownership, and
exhausted-recovery outcomes become manual-attention stops.

When status or failure writeback reports `validation_repeat`, `no_effective_diff`,
`remaining_delta_unchanged`, `review_churn`, `review_handoff_state_drift`,
`dependency_program_stale`, `uncovered_direction`, or `ambiguous_retained_progress`,
operators must inspect the retained worktree, private evidence, blocker state,
recovery packet, boundary check, review findings, or retained lifecycle record named
by that reason before clearing `decodex:needs-attention`.
Do not use steer, retry, label cleanup, or hard interrupt to bypass the guardrail
without changing the underlying repair strategy, dependency readiness, research
contract, authority decision, or retained-progress ownership decision.

Agents must not simulate manual attention by editing tracker state directly. The valid
agent path is:

1. request the configured `decodex:needs-attention` label through `issue_label_add`
2. call `issue_comment` with `kind = "manual_attention"` and structured public fields so Decodex can validate the blocker and apply the label
3. call `issue_terminal_finalize(path = "manual_attention")`

Operators may later clear the blocker, clear the label, and requeue the lane through
the configured lifecycle.

## Audit And Privacy

Every supported control mutation should create local runtime evidence. At minimum, a
control audit record should identify the project, issue, run id, attempt, branch,
operator command source, requested capability, normalized result, and next action.
Rejected control evidence must also preserve queue lease state, execution liveness,
process id/aliveness when observed, active channel path/status when present, current
thread/turn ids, retained worktree path, and latest protocol event summary.

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

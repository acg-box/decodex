# Linear Execution Ledger

Purpose: Define the versioned Linear comment records that mirror coarse Decodex lane
transitions for team visibility.
Status: normative
Read this when: You are implementing, reviewing, or consuming structured Linear
comments for Decodex team-visible lifecycle summaries.
Not this document: The local runtime state machine, operator status snapshot shape,
runtime SQLite schema, GitHub review orchestration, or repository validation gate.
Defines: The Linear execution event record envelope, event types, required and
optional fields, idempotency rules, retention expectations, and the boundary between
Linear comments, the Decodex runtime database, and short-lived heartbeat markers.

## Authority and scope

- Linear comments are the team-visible mirror for low-frequency Decodex lane
  transitions.
- Each ledger record is one Linear issue comment that contains one structured event.
- Ledger records describe durable transitions and handoff points, not high-frequency
  runtime telemetry.
- The ledger is append-only for normal operation. New facts use new event records
  instead of mutating earlier records.
- The schema in this document is the only authoritative schema for Decodex Linear
  execution event records.
- Fine-grained runtime truth lives in the Decodex runtime SQLite database and operator
  snapshots. Decodex must not rebuild active runtime state from Linear comments on
  every poll tick.
- Runtime behavior remains governed by [`runtime.md`](./runtime.md),
  [`post-review-lifecycle.md`](./post-review-lifecycle.md), and
  [`tracker-tools.md`](./tracker-tools.md). Those documents define when events may be
  written; this document defines what the records look like.
- Decision Contracts and internal Execution Programs are not Linear execution-ledger
  records. A ledger record may summarize or link to generated issues after promotion,
  but the versioned `decodex.decision_contract/1`,
  `decodex.execution_program/1`, and private loop evidence payloads stay in runtime
  SQLite.

## Comment body format

A Linear execution event comment must contain exactly one fenced JSON object whose
payload conforms to this schema. Human-readable text may surround the JSON block, but
the JSON object is the authoritative record.

Recommended shape:

````text
Decodex execution event: review_handoff

```json
{
  "record_type": "decodex.linear_execution_event",
  "record_version": 1,
  "event_type": "review_handoff",
  "event_timestamp": "2026-04-29T10:15:30Z",
  "idempotency_key": "decodex:XY-352:xy-352-attempt-1-1777430188:1:review_handoff:6f3d2a9",
  "service_id": "decodex",
  "issue_id": "a1b2c3d4",
  "issue_identifier": "XY-352",
  "run_id": "xy-352-attempt-1-1777430188",
  "attempt_number": 1,
  "branch": "y/decodex-xy-352",
  "worktree_path": ".worktrees/XY-352",
  "pr_url": "https://github.com/hack-ink/decodex/pull/123",
  "pr_head_sha": "0123456789abcdef0123456789abcdef01234567",
  "pr_base_ref": "main",
  "commit_sha": "0123456789abcdef0123456789abcdef01234567",
  "summary": "Documented the Linear execution ledger contract."
}
```
````

Consumers must ignore prose outside the fenced JSON object except for display.
Producers must not put secrets, access tokens, absolute host paths, or local user names
in ledger records.

## Public text baseline

Linear comments are public/team-visible tracker text. Before Decodex serializes a new
Linear execution event, public free-text fields such as `summary`, `next_action`,
`blockers`, `evidence`, `failed_command`, and `raw_error` must pass the baseline
public-text guard. The guard rejects known structured leakage shapes, including
host-local paths, routed identity configuration details, credential-like names, private
account details, private config file names, emails, tokens, and secrets. Current
`progress_checkpoint` events must render a public projection instead of copying raw
checkpoint `focus`, `next_action`, `blockers`, `evidence`, or `verification` text.

The guard is a baseline structural stop, not the final privacy boundary. Full runtime
evidence, local identity routing, account state, and high-frequency diagnostics must
stay in the local runtime database, operator-only evidence files, logs, or short-lived
activity markers. Linear records should continue to use public collaboration
identifiers such as PR URLs, issue identifiers, branch names, commit SHAs, and
repository-relative paths.

PR lifecycle writeback must not copy an agent-authored review, repair, or closeout
summary into Linear when that summary fails the baseline public-text guard. The
writeback must replace the rejected summary with fixed public-safe fallback text before
rendering the Linear comment and ledger record. This fallback does not weaken the guard:
the rejected private text remains absent from the public record.

Decodex may additionally run public projection free-text through an optional local
privacy classifier before publishing the Linear comment. That classifier is a secondary
semantic warning layer after schema allowlisting and the deterministic public-text
guard. It must receive only fields already selected for the public projection, never
the private runtime ledger or full checkpoint payload. If the configured local
classifier reports suspicious text or is unavailable, Decodex must fail closed by
omitting optional public text fields or replacing required public text fields with a
fixed public-safe summary.

Agent-requested manual-attention comments are not arbitrary Linear comment bodies.
They are `needs_attention` ledger records rendered from the allowlisted
`issue_comment` kind `manual_attention`. The agent supplies only structured public
fields. `failed_command` and `raw_error` are optional and must be omitted or rejected
when they contain host-local paths, credential-like names, private identity details,
tokens, secrets, or other private runtime evidence.

Linear frequency is deliberately sparse. A lane should normally create one start
record, a new public progress projection only when the material public lifecycle signal
changes, PR/handoff records when review state changes, and terminal failure, landing,
closeout, or cleanup records at those coarse boundaries. Private-only updates such as a
new checkpoint focus, next action, evidence item, verification note, raw command output,
heartbeat, token pressure, or retry detail belong in runtime SQLite, agent evidence, or
diagnostic logs, not in another Linear comment.

## Record envelope

All field names are snake_case.

| Field | Required | Type | Rule |
| --- | --- | --- | --- |
| `record_type` | yes | string | Must equal `decodex.linear_execution_event`. |
| `record_version` | yes | integer | Must equal `1` for this schema. |
| `event_type` | yes | string | Must be one of the event types in this document. |
| `event_timestamp` | yes | string | RFC 3339 timestamp in UTC, recorded when the event happened. |
| `idempotency_key` | yes | string | Stable key used to collapse duplicate records for the same event. |
| `service_id` | yes | string | The registered project config `service_id` that owns the lane. |
| `issue_id` | yes | string | The tracker issue id used by Linear APIs and local leases. |
| `issue_identifier` | yes | string | The human-visible Linear identifier such as `XY-352`. |
| `run_id` | yes | string | The Decodex run id for this attempt. |
| `attempt_number` | yes | integer | The 1-based attempt number for `run_id`. |

Ledger records are run-bound. If Decodex detects a candidate issue before it has a
`run_id` and `attempt_number`, that pre-run observation is not a Linear execution
ledger record.

## Shared optional fields

These fields are optional globally and become required for specific event types below.

| Field | Type | Rule |
| --- | --- | --- |
| `branch` | string | Lane branch name when the branch exists or is the event subject. |
| `worktree_path` | string | Repository-relative lane path when a worktree exists or is the event subject. Absolute paths are invalid. |
| `commit_sha` | string | Git commit SHA when the event is tied to a source revision, merge commit, or validated head. |
| `pr_url` | string | GitHub pull request URL when a PR exists or is the event subject. |
| `pr_head_sha` | string | PR head commit SHA when a PR exists or is the event subject. |
| `pr_base_ref` | string | PR base ref name when a PR exists or is the event subject. |
| `summary` | string | Short human-readable summary of the event. |
| `validation_result` | string | Repo-gate or PR validation result when validation is the event subject. |
| `phase` | string | Public execution-state phase for progress checkpoint records. |
| `focus` | string | Legacy progress focus field or private-runtime-only checkpoint input; current progress projections must not emit it. |
| `next_action` | string | Next execution action for failure records; current progress projections must not emit raw checkpoint next-action text. |
| `blockers` | array of strings | Concrete blockers, empty when none are present. |
| `evidence` | array of strings | Short factual evidence items. |
| `verification` | array of strings | Verification commands or checks already run. |
| `error_class` | string | Normalized failure class for needs-attention or terminal-failure records. |
| `terminal_path` | string | Explicit terminal path such as `review_handoff`, `review_repair`, `manual_attention`, or `retained_partial_progress`. |
| `cleanup_status` | string | Cleanup result when cleanup is the event subject. |
| `transport` | string | Agent transport name when agent startup is the event subject. |
| `target_state` | string | Tracker workflow state written by closeout or failure handling. |
| `failed_command` | string | Command that failed when a failure record is command-related. |
| `raw_error` | string | Short raw error text when it is needed to make a failure actionable. |

Optional fields must be omitted when unknown. Producers must not emit placeholder values
such as `unknown`, `n/a`, or empty strings for fields that are not known. Fields not
defined in this document are invalid for `record_version = 1`.

## Event types

The event type set is intentionally small and low-frequency:

- `run_started`
- `progress_checkpoint`
- `pr_opened`
- `pr_updated`
- `review_handoff`
- `review_handoff_rebind`
- `repair_handoff`
- `landed`
- `closeout`
- `needs_attention`
- `terminal_failure`
- `cleanup_complete`

No other `event_type` value is valid for new `record_version = 1` writes.
Historical startup records with `event_type` values `intake`, `lease_acquired`,
`worktree_prepared`, and `agent_started` remain valid for old comments, but current
Decodex writers must emit one `run_started` record instead of those separate startup
records.

## Event-specific fields

Every event requires the record envelope. Additional required fields are listed below.

| Event type | Additional required fields | Common optional fields |
| --- | --- | --- |
| `run_started` | `branch`, `worktree_path`, `commit_sha`, `transport`, `summary` |  |
| `intake` | `summary` | `branch`, `worktree_path`; legacy read-only startup record |
| `lease_acquired` | `branch` | `worktree_path`, `summary`; legacy read-only startup record |
| `worktree_prepared` | `branch`, `worktree_path`, `commit_sha` | `summary`; legacy read-only startup record |
| `agent_started` | `branch`, `worktree_path` | `transport`, `summary`; legacy read-only startup record |
| `progress_checkpoint` | `phase`, `summary` | `branch`, `worktree_path`, `pr_url` |
| `pr_opened` | `branch`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha` | `worktree_path`, `summary` |
| `pr_updated` | `branch`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha` | `worktree_path`, `summary` |
| `review_handoff` | `branch`, `worktree_path`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha`, `validation_result`, `summary`, `terminal_path` | `verification` |
| `review_handoff_rebind` | `branch`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha`, `validation_result`, `summary`, `evidence` | `worktree_path`, `next_action` |
| `repair_handoff` | `branch`, `worktree_path`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha`, `validation_result`, `summary`, `terminal_path` | `verification` |
| `landed` | `branch`, `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha`, `summary` | `worktree_path` |
| `closeout` | `pr_url`, `commit_sha`, `summary` | `branch`, `worktree_path`, `validation_result`, `target_state` |
| `needs_attention` | `error_class`, `next_action`, `blockers`, `evidence`, `terminal_path` | `branch`, `worktree_path`, `pr_url`, `commit_sha`, `failed_command`, `raw_error`, `summary` |
| `terminal_failure` | `error_class`, `next_action`, `blockers`, `evidence` | `branch`, `worktree_path`, `pr_url`, `commit_sha`, `failed_command`, `raw_error`, `summary` |
| `cleanup_complete` | `branch`, `worktree_path`, `cleanup_status`, `summary` | `pr_url`, `commit_sha` |

`terminal_path` values must match the runtime-owned terminal path for the tool or phase
that writes the event. For normal review handoff this is `review_handoff`; for retained
repair completion this is `review_repair`; for explicit human-required exits this is
`manual_attention`; for stalled dirty-worktree recovery this is
`retained_partial_progress`.

Retained partial progress is a `needs_attention` event with
`error_class = "partial_progress_retained"` and
`terminal_path = "retained_partial_progress"`. It must describe retained tracked
worktree changes and must not be emitted as `terminal_failure`. If the retained
disposition absorbs a later runtime failure, the producer should preserve the source
failure class in `evidence` instead of changing the event type or terminal path.

Loop guardrail stops are public `needs_attention` or `terminal_failure` records with
the runtime-owned `terminal_path = "manual_attention"` unless retained partial
progress has already taken precedence. The public record may use these normalized
`error_class` values: `validation_repeat`, `no_effective_diff`,
`remaining_delta_unchanged`, `review_churn`, `dependency_program_stale`,
`uncovered_direction`, or `ambiguous_retained_progress`. The Linear record must carry
only the public reason and next action; fingerprints, full checkpoint payloads,
review details, and worktree diagnostics remain in runtime SQLite private execution
events and `loop_guardrail_checkpoints`.

`failed_command` and `raw_error` are public-summary fields, not private evidence
escape hatches. Producers must validate those values before writing a Linear comment.
When the exact failed command or raw error contains private information, producers must
omit it and use public `error_class`, `next_action`, `blockers`, and `evidence`
instead.

`review_handoff_rebind` is only for an explicit operator recovery command that restores a
missing runtime DB review handoff marker after validating the retained worktree and PR
lineage. It is not a normal agent terminal signal, does not imply `issue_terminal_finalize`
ran, and must not be emitted automatically from `decodex run`.

## Progress checkpoint records

`progress_checkpoint` records are the Linear public projection of private durable
execution memory. They expose low-frequency lifecycle progress without changing
lifecycle authority. The full checkpoint payload from `issue_progress_checkpoint`
lives in private runtime execution events, not in Linear.

Required `phase` values are the same normalized phases accepted by
`issue_progress_checkpoint`:

- `probing`
- `implementing`
- `verifying`
- `blocked`
- `ready_for_review`
- `review_repair`
- `ready_to_land`
- `closeout`

`progress_checkpoint` records must not be interpreted as review handoff, repair
completion, merge readiness, closeout, cleanup completion, or terminal success. Those
transitions require their dedicated event type and the governing runtime/tool contract.

Current progress projections must contain only the allowlisted public fields in the
event table above. They must not emit raw `focus`, `next_action`, `blockers`,
`evidence`, `verification`, local head evidence, host-local paths, routed identity
details, account details, token names, or other private runtime evidence. Producers must
render a short public `summary` from the public lifecycle signal, for example the
normalized phase, instead of copying agent-authored checkpoint prose.

Progress projection idempotency is anchored to the material public signal, such as the
normalized phase plus public branch/worktree/PR projection anchor. Retrying a checkpoint
or adding new private focus, next-action, evidence, blocker, or verification details
inside the same public signal must append private runtime evidence without adding a new
Linear comment.

## Ledger-only comment contract

Decodex writes and reads durable execution outcomes through
`decodex.linear_execution_event` records only. Structured checkpoint,
review-handoff, closeout, or other non-ledger payloads are issue history only.

Runtime recovery must continue to use the runtime database, retained worktree
markers, and current tracker/PR state as its active authority. Non-ledger Linear
comments must not hydrate Run Ledger outcomes, satisfy a missing execution ledger, or
be replayed as the active state machine.

## Idempotency and ordering

- `idempotency_key` must be deterministic for the logical event.
- Retrying the same write must reuse the same `idempotency_key`.
- Writing a materially new transition, checkpoint, PR head, failure, or cleanup result
  must use a new `idempotency_key`.
- Consumers must de-duplicate records with the same `record_type`, `record_version`,
  `service_id`, `issue_id`, and `idempotency_key`.
- If duplicates disagree, consumers should prefer the earliest valid record and surface a
  data-quality warning instead of guessing which record is authoritative.
- Producers that perform side effects for a `needs_attention` or `terminal_failure`
  record must treat the idempotency key as guarding the whole writeback. A duplicate
  terminal event in the local runtime store or the remote Linear comment ledger must
  not reapply the tracker state transition, automation-label mutations, or public
  comment.
- Event ordering is by `event_timestamp`, with Linear comment creation time as a
  fallback tiebreaker. Consumers must tolerate delayed comments and duplicate retries.

Recommended idempotency shape:

```text
<service_id>:<issue_identifier>:<run_id>:<attempt_number>:<event_type>:<stable-anchor>
```

The `stable-anchor` should be the most specific durable anchor for the event, such as a
commit SHA, PR head SHA, terminal path, or checkpoint sequence key.

## Linear comments versus runtime state

Use Linear comments for team-visible, low-frequency records:

- lane run start, including branch, worktree, current commit, and transport
- public progress checkpoint projections
- PR opened or updated events
- review handoff and retained repair handoff
- landed, closeout, needs-attention, terminal failure, and cleanup-complete events
- stable identity and recovery fields: `service_id`, `issue_id`,
  `issue_identifier`, `run_id`, `attempt_number`, `branch`, `worktree_path`,
  `pr_url`, `pr_head_sha`, `pr_base_ref`, `commit_sha`, `event_timestamp`, and
  `idempotency_key`

Use the Decodex runtime database and `.decodex-run-activity` for local/operator-only
runtime telemetry:

- heartbeat timestamps and current operation updates
- app-server protocol event counts and last event names
- thread liveness, wait reasons, idle seconds, retry timers, and suspected-stall hints
- `child_agent_activity` buckets
- token counts, largest tool-output sizes, and context-pressure warnings
- review-policy convergence counters that only guide the current retained-lane retry
  loop
- full `issue_progress_checkpoint` payloads, including raw focus, next action,
  blockers, evidence, verification, and local head evidence
- transient diagnostic details that help the local operator understand whether an active
  process is busy, idle, or stalled

High-frequency heartbeat, child-agent buckets, token counts, and transient idle details
must stay local/operator-only. They must not be promoted into Linear execution ledger
comments, because they would turn Linear into a noisy telemetry sink rather than the
team-visible execution ledger.

If a field is required for team-visible issue history, it belongs in a Linear ledger
record. If a field is required for local scheduling, recovery, retry ownership, phase
timing, dashboard freshness, or agent liveness, it belongs in the runtime database or
`.decodex-run-activity`.

Operator status and dashboard consumers may aggregate ledger records for completed
history lanes that are already present in the local runtime attempt window. That
aggregation is display-only: it may show PR URL, landed or merge commit, closeout
status, needs-attention reason, and elapsed lifecycle timing from Linear comments, but
it must not replay those comments as active leases, dispatch ownership, retry state, or
post-review orchestration authority.

Successful closeout and cleanup results must remain successful in local runtime history.
If a closeout or cleanup child exits successfully, status consumers should surface the
lane as `completed` or the run attempt as `succeeded`, even when the Linear tracker
issue was already `Done` before the child exited. A pre-existing terminal tracker state
must not downgrade an observed successful closeout or cleanup to `terminated`.

## Retention expectations

- Linear ledger comments are retained with the Linear issue for the lifetime of that
  issue.
- Decodex must not delete ledger comments during normal cleanup.
- Redaction is allowed only for accidental secret or host-private data exposure, and the
  replacement comment must preserve the original `idempotency_key` plus a short redaction
  reason.
- Runtime database rows are owned by the local Decodex installation and retained by
  explicit cleanup policy.
- Local `.decodex-run-activity` markers are short-lived runtime state. They may be
  updated frequently, replaced by newer state, or removed during deterministic cleanup.
- Removing local markers must not erase the team-visible execution ledger because the
  durable lane transition records remain in Linear comments.

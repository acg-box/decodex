# Tracker Tool Specification

Purpose: Define the issue-scoped tracker tool surface that allows the coding agent to update the currently leased issue autonomously while keeping `decodex` in control of orchestration lifecycle and safety.
Status: normative
Read this when: You are implementing, reviewing, or constraining the issue-scoped tracker tool bridge used during a `decodex` run.
Not this document: The full `decodex` runtime state machine, the downstream `WORKFLOW.md` contract, or the end-to-end pilot runbook.
Defines: The tracker ownership boundary, preferred transport, issue-scoped tool surface, policy constraints, and failure-handling rules for tracker writes.

## Relationship to execution ledger records

- [`linear-execution-ledger.md`](./linear-execution-ledger.md) defines the versioned
  Linear comment event-ledger schema for Decodex lane transitions.
- This document defines which issue-scoped tools may write tracker state and when those
  writes are valid.
- Tool calls that create durable Decodex execution comments must use the ledger schema
  instead of ad hoc record-specific comment formats.

## Ownership boundary

- The coding agent should normally perform tracker writes for the currently leased issue during the run.
- `decodex` still owns:
  - lease acquisition and release
  - worktree lifecycle
  - retries and retry budget enforcement
  - startup reconciliation
  - crash recovery and terminal fallback writes
- The coding agent must not gain broad tracker write access beyond the currently leased issue.

## Preferred transport

- Preferred follow-up transport: a client-side dynamic tool bridge handled inside the existing `app_server` JSON-RPC client.
- Evidence source: the local `codex app-server generate-json-schema` bundle exposes server-driven dynamic tool call requests (`item/tool/call`) and related tool-call notifications.
- Deferred alternative: a process-local MCP server may be introduced later if the required tool surface grows beyond what the dynamic bridge can represent safely.

## Scope model

- Every run attempt leases exactly one tracker issue.
- The tool bridge must bind the agent to that single leased issue identifier.
- Tool calls that reference any other issue must be rejected.
- Tool calls that request unsupported operations must be rejected.

## Minimum tool surface

The follow-up MVP should support these issue-scoped operations:

- `issue_transition`
  - move the current issue to an allowed target state
- `issue_comment`
  - add an exceptional human-readable comment to the current issue for
    manual-attention blockers or explicit collaboration notes
- `issue_progress_checkpoint`
  - record the current durable execution-state snapshot for the current issue without changing lifecycle authority
- `issue_review_checkpoint`
  - record the normalized repo-native bounded-review result for the current handoff or repair phase
- `issue_review_handoff`
  - validate and record a PR-backed success handoff for the current issue
- `issue_label_add`
  - add a label to the current issue when workflow policy requires it
- `issue_terminal_finalize`
  - explicitly finalize the current run's terminal tracker path after the required tracker writes already exist

Additional operations such as richer metadata updates may be added later, but they are not required for the first PR-backed self-dogfood pilot.

## Completion signal contract

At turn completion, the issue-scoped tool bridge must leave `decodex` with exactly one terminal completion signal for the leased issue and a matching explicit terminal-finalization call:

- `review_handoff`
  - produced by `issue_review_handoff`
  - finalized by `issue_terminal_finalize(path = "review_handoff")`
  - means the lane is claiming review-ready success
- `manual_attention`
  - produced by adding the configured `needs_attention_label` and leaving an explanatory comment
  - finalized by `issue_terminal_finalize(path = "manual_attention")`
  - means the lane is explicitly handing the issue back to a human instead of asking for `In Review`

Invalid outcomes:

- both signals are present
- neither signal is present
- a signal is present, but the matching `issue_terminal_finalize` call never happened
- `issue_terminal_finalize` names a different path than the currently recorded terminal signal

In either invalid case, `decodex` must fail the attempt rather than infer which path the agent intended.

## Policy constraints

- Allowed target states should be constrained by repo workflow policy plus the orchestration phase.
- The tool bridge should reject transitions that violate the current repo workflow contract.
- Generic `issue_transition` must not move the current issue directly into the configured success state.
- `issue_progress_checkpoint` is available during any owned run phase, including retained repair and closeout runs.
- `issue_progress_checkpoint` must keep the routed issue description generic; the durable execution-state payload belongs in issue-scoped checkpoint comments, not in the description.
- `issue_progress_checkpoint` must accept only the normalized execution phases `probing`, `implementing`, `verifying`, `blocked`, `ready_for_review`, `review_repair`, `ready_to_land`, and `closeout`.
- `issue_progress_checkpoint` must not replace `issue_review_checkpoint`, `issue_review_handoff`, `issue_review_repair_complete`, `issue_closeout_complete`, or `issue_terminal_finalize`.
- `decodex` treats `issue_progress_checkpoint` as execution memory only. Checkpoint phase, focus, next action, blockers, or evidence do not by themselves authorize review handoff, repair completion, merge, closeout, or terminal success.
- `issue_review_checkpoint` is available only when `codex.internal_review_mode = "loop"`, and only during the pre-PR handoff phase and retained review-repair runs; `closeout` does not expose it.
- `issue_review_checkpoint` must accept only these normalized statuses: `clean`, `findings`, `needs_architecture_review`, `blocked`.
- `issue_review_checkpoint` must bind every checkpoint to an explicit `head_sha` for the currently reviewed lane head.
- When `codex.internal_review_mode = "loop"`, `decodex` treats `issue_review_checkpoint` as the only authoritative structured review-policy signal. Skill prose or wrapper-local result words must not replace it.
- When `codex.internal_review_mode = "loop"`, `issue_review_handoff` and `issue_review_repair_complete` must require the latest `clean` checkpoint for the current phase and current lane head, not merely any older clean checkpoint from the same lane.
- When `codex.internal_review_mode = "prompt"` or `"off"`, `issue_review_handoff` and `issue_review_repair_complete` must not require `issue_review_checkpoint`; they still must pass PR validation, branch/head checks, and the configured repository validation gate before writeback.
- `issue_review_handoff` must validate that the supplied PR belongs to the current repository and lane branch, points at the validated lane HEAD, is open, and is ready for review before `decodex` accepts the handoff.
- `issue_review_repair_complete` must validate that the supplied PR belongs to the current repository and retained lane branch, points at the validated lane HEAD, is open, and is ready for fresh review before `decodex` accepts retained repair completion.
- `issue_review_handoff` records the success metadata during the turn, but `decodex` owns the final completion comment and `In Review` transition after service-side validation succeeds.
- `issue_review_repair_complete` records retained repair completion metadata during the turn, but `decodex` owns the final completion comment and refreshed retained-lineage marker after service-side validation succeeds.
- Adding the configured `needs_attention_label` is an explicit human-required failure exit for the active lane. In that case the agent must leave a comment explaining the blocker, must not also record `issue_review_handoff`, and `decodex` must stop automatic retries for that attempt.
- Human-attention comments must describe the exact observed blocker and should include the failed command plus raw error text when available. The agent must not speculate about capabilities or environment restrictions that it did not directly verify.
- The human-attention exit is not complete until the explanatory comment is successfully written after the label request. A label-only signal must be rejected as an invalid completion disposition.
- The run is not complete until `issue_terminal_finalize` succeeds against the matching terminal path. An execution-state checkpoint or an agent summary message is not a substitute.
- Issues that carry the configured `needs_attention_label` must remain ineligible for future automatic selection until a human clears the label.
- `issue_review_handoff` and the human-attention exit are mutually exclusive terminal signals for the same turn.
- Generic live dispatch for a startable issue must not require GitHub CLI authority before the lane actually attempts a PR-backed review handoff.
- `decodex` must resolve the configured GitHub token before launching the agent app-server, so lane Git and PR-creation commands inherit noninteractive credentials; missing or blank credentials are human-required terminal failures, not retryable promptable runs.
- `decodex` must preflight the local GitHub CLI dependency at the PR-backed review boundary itself:
  - when a normal lane is about to validate and write back `issue_review_handoff`
  - when a retained post-review lane is about to re-enter `review_repair`
- Comment bodies should remain repository-controlled or agent-authored, but all tool calls must be journaled by `decodex` for recovery and audit.
- Routine start and progress visibility should use Linear execution ledger records
  instead of ad hoc `issue_comment` text. A normal run start is represented by one
  `run_started` ledger record, and ordinary progress uses `issue_progress_checkpoint`
  only when execution phase, focus, next action, blockers, evidence, or verification
  changes materially.
- Structured Linear execution event comments must conform to
  [`linear-execution-ledger.md`](./linear-execution-ledger.md).
- Structured comment fields such as `worktree_path` must use repository-relative paths;
  absolute host paths should be rejected before writing to the tracker.
- `issue_comment` and `issue_progress_checkpoint` text is public/team-visible. Before
  either tool writes to Linear, Decodex must reject known leakage-shaped text such as
  host-local paths, routed identity details, credential-like names, private account
  details, private config file names, emails, tokens, or secrets. This baseline guard
  does not replace the longer-term local-private ledger boundary; detailed runtime
  evidence remains local/operator-only.
- Dynamic tool names must satisfy the `codex app-server` identifier restriction `^[a-zA-Z0-9_-]+$`; dotted names are invalid.

## Failure handling

- If the agent never reaches a tracker write, `decodex` may perform a minimal fallback write during reconciliation or terminal failure handling.
- If a tracker tool call fails transiently, the failure should be surfaced to the run journal so retry logic can reason about it.
- If a tracker tool call fails because it targeted the wrong issue or an unsupported operation, treat that as a policy violation, not as a retryable transport error.
- When `codex.internal_review_mode = "loop"`, if the latest `issue_review_checkpoint` reports `findings` for the third consecutive non-clean round on the same phase, or reports `needs_architecture_review` / `blocked`, `decodex` must stop the lane through the human-required failure path instead of retrying automatically.
- Review-policy stops do not dispatch research directly. `decodex` may surface
  operator guidance for a bounded research follow-up, but future runtime-owned research
  escalation is valid only after a separate adapter contract can verify the current
  head, review phase, normalized stop kind, normalized error class, issue/run identity,
  and latest bounded-review evidence.
- If the turn completes without a valid recorded `issue_review_handoff` and without an explicit human-attention exit, `decodex` must treat the run as failed rather than silently moving the issue to `In Review`.
- If the turn completes without a matching `issue_terminal_finalize` call for the resolved terminal path, `decodex` must treat the run as failed before reporting the attempt as successful.
- If PR-backed success writeback partially succeeds, for example the issue reaches `In Review` but the completion comment fails to post, `decodex` must treat the lane as human-required and must not place it back on the automatic retry path.

## Future expansion

- A later phase may lift the transport from a dynamic tool bridge to a process-local MCP server if broader tracker or repo-collaboration tools are required.
- Any future expansion must preserve the issue-scoped safety boundary unless the user explicitly approves a broader trust model.

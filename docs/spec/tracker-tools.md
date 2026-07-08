---
type: "Spec"
title: "Tracker Tool Specification"
description: "Define the issue-scoped tracker tool surface that allows the coding agent to update the currently leased issue autonomously while keeping `decodex` in control of orchestration lifecycle and safety. Status: normative Read this when: You are implementing, reviewing, or constraining the issue-scoped tracker tool bridge used during a `decodex` run. Not this document: The full `decodex` runtime state machine, the downstream `WORKFLOW.md` contract, or the end-to-end pilot runbook. Defines: The tracker ownership boundary, preferred transport, issue-scoped tool surface, policy constraints, and failure-handling rules for tracker writes."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/agent/tracker_tool_bridge.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/agent/tracker_tool_bridge/review.rs, apps/decodex/src/orchestrator/execution.rs]
drift_watch: [issue_progress_checkpoint, issue_review_checkpoint, issue_review_handoff, issue_review_repair_complete, review_contract, review_cost_control, validation_evidence, issue_terminal_finalize, docs_impact, private_execution_events, linear_execution_event]
last_verified: 2026-06-23
---
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

## Minimum agent tool surface

The follow-up MVP should support these issue-scoped operations:

- `issue_transition`
  - move the current issue to an allowed target state
- `issue_comment`
  - add an allowlisted public comment summary to the current issue for a known
    lifecycle case
  - current automation kind: `manual_attention`
  - the tool accepts structured public fields and renders the Linear comment itself;
    it must not accept arbitrary agent-authored comment bodies
- `issue_progress_checkpoint`
  - append the current durable execution-state snapshot to private runtime evidence and publish only the low-frequency public projection when the public lifecycle signal changes
- `issue_review_handoff`
  - validate and record a PR-backed success handoff for the current issue
- `issue_label_add`
  - add an allowlisted immediate label to the current issue when workflow policy
    requires it, or record a run-local manual-attention label intent for the
    configured `needs_attention_label`
- `issue_terminal_finalize`
  - explicitly finalize the current run's terminal tracker path after the required tracker writes already exist

`issue_review_checkpoint` is a runtime-owned evidence writer, not an agent-facing
tool. Decodex may use that internal path after PR-backed handoff or retained repair
completion to persist the normalized bounded-review result for the current phase.

Additional operations such as richer metadata updates may be added later, but they are not required for the first PR-backed self-dogfood pilot.

## Private And Public Outputs

Tracker tools must keep local execution evidence private by default:

- `issue_progress_checkpoint` stores the full normalized checkpoint payload in
  runtime SQLite `private_execution_events` before any Linear write is attempted.
- Its Linear output is only the public `progress_checkpoint` projection defined by
  [`linear-execution-ledger.md`](./linear-execution-ledger.md). That projection is
  keyed by public lifecycle signal, so private-only checkpoint changes do not create
  another Linear comment.
- `issue_comment` is not a generic comment escape hatch. It accepts only allowlisted
  public comment kinds, currently `manual_attention`, and Decodex renders the
  corresponding Linear ledger record from structured public fields.
- Logs and `.decodex-run-activity` markers are diagnostic inputs for local operators.
  They must not be copied into Linear through tracker tools and must not replace
  private execution events.

## Completion signal contract

At turn completion, the issue-scoped tool bridge must leave `decodex` with exactly one terminal completion signal for the leased issue and a matching explicit terminal-finalization call:

- `review_handoff`
  - produced by `issue_review_handoff`
  - finalized by `issue_terminal_finalize(path = "review_handoff")`
  - means the lane is claiming review-ready success
- `manual_attention`
  - produced by requesting the configured `needs_attention_label`, leaving a
    validated explanatory comment, and having Decodex apply the label before writing
    the comment
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
- `issue_progress_checkpoint` must keep the routed issue description generic. The full
  structured checkpoint payload belongs in private runtime execution events, not in
  the issue description or Linear comments.
- Accepted Decision Contracts are local runtime records, not tracker
  tool comments or issue-description payloads. Tracker tools may later publish sparse
  public projections or generated issue links after acceptance, but they must not copy
  the private `decodex.decision_contract/1` payload into Linear.
- Internal Execution Programs are local runtime records, not tracker comments or
  issue-description payloads. Program dispatch is direct: the scheduler evaluates
  ready nodes mapped to normal startable Linear issues, refreshes only the tracker
  facts required for readiness, and dispatches with `program` dispatch mode. Tracker
  label mutation tools must not apply, retain, or remove
  `decodex:queued:<service-id>` for Program readiness. Service queue labels remain
  the ordinary intake signal for non-Program issue lanes.
- `decodex intake issues ... --dry-run` may read tracker state for supplied issues
  and render a local ready/held/blocked/stale/unmapped report, but it must not call
  tracker label mutation tools or write Linear comments. `--apply` may write local
  runtime Program Intake records and issue mappings, but it still must not apply or
  remove `decodex:queued:<service-id>`.
- `issue_progress_checkpoint` must accept only the normalized execution phases `probing`, `implementing`, `verifying`, `blocked`, `ready_for_review`, `review_repair`, `ready_to_land`, and `closeout`, plus the docs-impact enum `none`, `update_required`, `research_required`, or `drift_required`.
- `issue_progress_checkpoint` must not replace `issue_review_checkpoint`, `issue_review_handoff`, `issue_review_repair_complete`, `issue_closeout_complete`, or `issue_terminal_finalize`.
- `decodex` treats `issue_progress_checkpoint` as execution memory only. Checkpoint phase, docs impact, focus, next action, blockers, or evidence do not by themselves authorize review handoff, repair completion, merge, closeout, or terminal success.
- For implementation and repair phase-goal transitions, Decodex records private
  validation evidence from the current worktree, repo gate, changed surfaces, and any
  current-HEAD `issue_progress_checkpoint`. A checkpoint remains execution memory and
  blocker evidence, but agents do not need to publish checkpoint comments solely to
  satisfy phase ceremony. Repo-gate pass alone is not transition authority: validation
  evidence must still prove an effective delta, non-goal status, changed surfaces,
  head identity, and any supplied docs-impact checkpoint before Decodex advances to
  the appropriate terminal-evidence phase (`handoff_evidence` or
  `review_repair_evidence`).
- Before `issue_terminal_finalize` can complete any terminal path, the latest private progress checkpoint for the run attempt must include parseable `docs_impact` and match the current lane `HEAD`.
- `issue_progress_checkpoint` must persist the full normalized checkpoint payload to
  `private_execution_events` before attempting any Linear write.
- The Linear-facing checkpoint record is only a public projection. It may include the
  ledger envelope, `phase`, `summary`, `branch`, repository-relative `worktree_path`,
  and `pr_url`. It must not include raw `focus`, `next_action`, `blockers`,
  `evidence`, `verification`, local head evidence, host-local paths, identity-routing
  details, account details, token names, or other private runtime evidence.
- When a project configures a local public-projection privacy classifier, Decodex must
  run only Linear projection text fields through that local classifier before writing
  the Linear comment. The classifier is not the primary boundary and must not receive
  raw checkpoint `focus`, `next_action`, `blockers`, `evidence`, `verification`,
  local runtime events, or other private ledger payloads.
- Suspicious or classifier-unavailable projection fields must fail closed: optional
  fields are omitted, and required text fields are replaced with fixed public-safe
  fallback text before any Linear mutation.
- `issue_progress_checkpoint` must publish a new Linear projection only when the
  public lifecycle signal changes materially, such as the normalized phase or public
  branch/PR projection anchor changing. Repeated private evidence updates inside the
  same public signal must append private runtime events without adding Linear comments.
- The runtime-owned `issue_review_checkpoint` evidence writer is enabled only when
  `[codex].review` is `"standard"` or `"strict"`, and only for the post-handoff or
  retained review-repair phase; no agent run exposes it in dynamic tool specs.
- `issue_review_checkpoint` must accept only these normalized statuses:
  `clean`, `findings`, `needs_architecture_review`, `blocked`.
- `issue_review_checkpoint` must bind every checkpoint to an explicit `head_sha`
  for the currently reviewed lane head and must fail closed unless review-blocking
  local changes are absent. Review-blocking changes include tracked changes and
  non-runtime untracked files; the only untracked source-tree runtime artifacts this
  gate may ignore are the top-level `.decodex-run-activity` marker and
  `.decodex-run-control/` directory. Formal Decodex Review evidence is evidence for
  a committed lane head, not for a dirty worktree snapshot.
- `issue_review_checkpoint` records the independent fresh-context read-only review
  result as structured runtime evidence. The reviewer source is
  `independent_fresh_context`; every new checkpoint payload must include a
  `review_contract` with `workflow_policy_source =
  "registered_project_workflow"`, a phase-correct `review_type`
  (`full_current_head_review` for handoff, `repair_verification` for repair),
  `risk_tier`, objective, scope, non-goals, required checks, allowed expansion
  triggers, and validation evidence. The persisted payload must also bind the
  reviewed `head_sha`, `head_tree_oid`, clean review-worktree fact, and stable
  `review_contract_hash`.
- A checkpoint may include `review_cost_control` with `review_class`
  (`compact_current_head_review` or `full_current_head_review`), `risk_class`,
  changed-surface count and public-safe summary, high-risk surface summary,
  current-head evidence flag, validation-backed flag, validation-current flag,
  evidence-sufficient flag, reviewer judgment, and fallback reason for full review.
  Compact review checkpoints keep the fallback reason absent, while full-review
  checkpoints must record the reason compact review was not selected. Omitted
  cost-control metadata normalizes to
  `full_current_head_review` with fallback reason
  `review_cost_control_not_provided`.
- `compact_current_head_review` is accepted only for a clean handoff checkpoint with
  `risk_tier = "low"`, `risk_class = "low"`, current-head evidence, validation
  evidence that is current for the reviewed `HEAD`, sufficient current-head
  evidence quality, a small changed-surface count, no high-risk surfaces, no
  accepted findings, no current or landing-blocking routes, and no prior non-clean
  checkpoint state for the same review phase. It is a compact independent review
  path, not a skipped-review path. Full review is required for repair verification,
  accepted findings, non-clean rounds, missing/stale validation, docs/config/API/
  security/data/privacy surfaces without sufficient evidence, weak evidence, or
  architecture risk.
- Every new checkpoint payload must include checklist notes for intended behavior,
  regression risk, missing tests, docs/config drift, migration fallout,
  operator-facing fallout, and Loop/Decision Contract mismatch.
- Review payload findings are split into accepted findings, rejected findings, and
  route evidence. Accepted and rejected finding arrays remain compatible with earlier
  callers, but new adjudication uses `finding_routes` to decide where each review
  signal belongs before repair. When `finding_routes` is omitted, accepted findings
  default to `current_blocker` and rejected findings default to
  `reviewer_rubric_gap`.
- `finding_routes.route` supports this taxonomy: `current_blocker`,
  `landing_blocker`, `contract_or_authority_decision_required`, `needs_evidence`,
  `follow_up`, `deterministic_gate_candidate`, `architecture_signal`,
  `issue_contract_gap`, `reviewer_rubric_gap`, `risk_note`, and
  `invalid_or_unsubstantiated`.
- Only accepted findings routed as `current_blocker` are current repair input. Only
  those current-blocker fingerprints drive `repair_accepted_review_findings`,
  `nonclean_rounds`, and review churn repeat counting. Non-current routes remain
  durable in local evidence and status readback but must not start repair churn.
  Rejected findings record the rejection reason for non-actionable, stale,
  out-of-scope, or unvalidated reviewer comments.
- A `findings` checkpoint must carry at least one accepted finding routed as
  `current_blocker`. A `clean` checkpoint may carry rejected findings and
  non-blocking routes such as
  `follow_up`, `risk_note`, `reviewer_rubric_gap`, or
  `invalid_or_unsubstantiated`, but must not carry accepted findings, current
  blockers, or landing-blocking routes.
- Top-level checkpoint evidence is required. Accepted findings must include severity,
  non-empty evidence, file and line or line-range references when possible, and
  concrete repair guidance. Rejected findings must include severity, non-empty
  evidence, and the rejection reason. Accepted and rejected findings may include a
  public snake_case `kind`; omitted kinds normalize to `accepted_finding` or
  `rejected_finding`.
- Each explicit `finding_routes` item must include route, severity, non-empty
  evidence, resolver, and machine-actionable `next_action`. A route may bind to an
  accepted or rejected finding by `finding_source` plus zero-based `finding_index`,
  or stand alone as `route_only`. `current_blocker` must bind to an accepted finding.
  `blocked` and `needs_architecture_review` checkpoints must include at least one
  landing-blocking route such as `landing_blocker`,
  `contract_or_authority_decision_required`, `needs_evidence`,
  `deterministic_gate_candidate`, `architecture_signal`, or `issue_contract_gap`.
  High-severity (`critical` or `high`) or explicitly high-risk routes must not use
  `invalid_or_unsubstantiated`; they must use `needs_evidence` or a
  landing-blocking route.
- Each accepted finding is normalized to a stable `review_finding:<sha256>`
  fingerprint from the review phase, finding kind, summary, guidance, file, and
  line range. The persisted review payload includes a `finding_policy` summary with
  active current-blocker fingerprints, per-fingerprint repeat counts, and an
  optional `stop_fingerprint`. The persisted payload also includes a compact route
  summary with route counts and one route-derived next action for local readback.
- When `[codex].review` is `"standard"` or `"strict"`, `decodex` treats the
  runtime-owned `issue_review_checkpoint` artifact as the only authoritative
  structured review-policy signal. Skill prose, wrapper-local result words, or
  agent-authored summaries must not replace it.
- When `[codex].review` is `"standard"` or `"strict"`, `issue_review_handoff` and
  `issue_review_repair_complete` record only the pushed PR lifecycle fact. Runtime
  post-review classification and retained orchestration must require the latest
  `clean` checkpoint for the current phase and current lane head before landing or
  proceeding on the clean path, not merely any older clean checkpoint from the same
  lane. They must also re-check that review-blocking local changes are still absent
  before using that checkpoint, so dirty edits after review cannot pass under the
  same `HEAD`.
- When `[codex].review` is `"off"`, `issue_review_handoff` and
  `issue_review_repair_complete` must not require `issue_review_checkpoint`; they
  still must pass PR validation, branch/head checks, and the configured repository
  validation gate before writeback.
- `issue_review_handoff` must validate that the supplied PR belongs to the current repository and lane branch, points at the validated lane HEAD, is open, is ready for review, and reads back as the same requested PR URL before `decodex` accepts the handoff.
- `issue_review_repair_complete` must validate that the supplied PR belongs to the current repository and retained lane branch, points at the validated lane HEAD, is open, and is ready for fresh review before `decodex` accepts retained repair completion.
- `issue_review_handoff` records a private `review_completion_intent` during the
  turn, but `decodex` owns the final completion comment and `In Review` transition
  after service-side validation succeeds. `issue_terminal_finalize(path =
  "review_handoff")` must revalidate the pending PR against that exact private
  intent, the retained worktree branch, the current local HEAD, and the PR head, then
  write the local `review_lifecycle_records` row before terminal finalize can
  succeed. If an existing lifecycle row for the lane branch points at a different PR
  URL, head ref, base ref, or head OID, the tool must fail closed and require explicit
  review-handoff recovery instead of rebinding implicitly.
- `issue_review_repair_complete` records retained repair completion metadata during the turn, but `decodex` owns the final completion comment and refreshed retained-lineage marker after service-side validation succeeds. For retained repair finalization, service-side validation includes pushing the validated local `HEAD` to the retained PR branch, surfacing typed push auth/refspec/remote-rejection failures before marker refresh, and then re-reading the PR so the refreshed retained-lineage marker is written only when the remote PR head matches the validated local `HEAD`.
- Agent-authored PR lifecycle summaries are public text inputs. If the summary recorded
  by `issue_review_handoff`, `issue_review_repair_complete`, or `issue_closeout_complete`
  fails the public-text guard during Decodex-owned writeback, Decodex must use fixed
  public-safe fallback summary text for the Linear comment and ledger record instead
  of failing the otherwise valid PR lifecycle transition.
- Calling `issue_label_add` with the configured `needs_attention_label` records an
  explicit human-required failure intent for the active lane, but it is not an
  immediate Linear mutation. In that case the agent must call `issue_comment` with
  kind `manual_attention` so Decodex can validate the blocker, apply the
  `needs_attention_label`, and render the explanatory `needs_attention` ledger
  comment. The agent must not also record `issue_review_handoff`, and `decodex` must
  stop automatic retries for that attempt only after the paired manual-attention exit
  validates.
- Human-attention comments must describe the exact observed blocker through
  structured public fields: `error_class`, `next_action`, `blockers`, and
  `evidence`. `failed_command` and `raw_error` may be included only when their
  values are public-safe. The tool must reject private-looking command or error
  text before any Linear mutation. The agent must not speculate about
  capabilities or environment restrictions that it did not directly verify.
- `manual_attention` must not use runtime-owned retry or continued-repair
  `error_class` values such as retryable app-server, stalled-run, or repo-gate
  validation classes. Those classes remain owned by Decodex retry, continuation, or
  architecture-recovery policy until the runtime itself reaches a human-required
  terminal boundary.
- For authority-boundary stops, `manual_attention` may include a structured
  `decision_request` object. Its Linear-rendered fields are public-safe only:
  `decision_request_id`, `reason_code`, `boundary_type`, `proposed_change`,
  `why_exceeds_authority`, options, recommendation, and `resume_condition`. Its
  private fields, including the Authority Boundary Check record id, retained
  worktree evidence, retained diff evidence, and recovery-attempt context, must be
  written to private runtime evidence before the Linear write and must not be
  rendered into the public comment.
- The human-attention exit is not complete until the requested label and explanatory
  comment are successfully written after the comment validates. A label-intent-only
  signal must be rejected as an invalid completion disposition, and invalid,
  private-looking, unsupported, or runtime-owned retryable comments must not leave a
  label-only attention state behind.
- The run is not complete until `issue_terminal_finalize` succeeds against the matching terminal path. An execution-state checkpoint or an agent summary message is not a substitute.
- Issues that carry the configured `needs_attention_label` must remain ineligible for future automatic selection until a human clears the label.
- `issue_review_handoff` and the human-attention exit are mutually exclusive terminal signals for the same turn.
- Generic live dispatch for a startable issue must not require GitHub CLI authority before the lane actually attempts a PR-backed review handoff.
- `decodex` must resolve the configured GitHub token before launching the agent app-server, so lane Git and PR-creation commands inherit noninteractive credentials; missing or blank credentials are human-required terminal failures, not retryable promptable runs.
- `decodex` must preflight the local GitHub CLI dependency at the PR-backed review boundary itself:
  - when a normal lane is about to validate and write back `issue_review_handoff`
  - when a retained post-review lane is about to re-enter `review_repair`
  - using the same resolved `gh` executable path that PR inspection will use, not a
    narrower lookup path for preflight than for `issue_review_handoff`
- Decodex execution comment bodies should be rendered by Decodex from
  structured, validated fields. All tool calls must be journaled by `decodex`
  for recovery and audit.
- Routine start and progress visibility should use Linear execution ledger records
  instead of ad hoc `issue_comment` text. A normal run start is represented by one
  `run_started` ledger record. Ordinary progress uses `issue_progress_checkpoint`
  when execution phase, docs impact, focus, next action, blockers, evidence, or
  verification changes materially, but Linear receives only the safe public
  projection for material public lifecycle changes.
- Structured Linear execution event comments must conform to
  [`linear-execution-ledger.md`](./linear-execution-ledger.md).
- Structured comment fields such as `worktree_path` must use repository-relative paths;
  absolute host paths should be rejected before writing to the tracker.
- `issue_comment` is public/team-visible but not free-form. It must accept only
  allowlisted public comment kinds and structured public fields. For
  `manual_attention`, Decodex renders a `needs_attention` Linear execution ledger
  comment from those fields. Unsupported kinds, arbitrary `body` arguments, and
  private-looking `failed_command` or `raw_error` values must be rejected before
  any Linear write. `issue_progress_checkpoint` payload text is private runtime
  evidence; only its rendered public projection is public/team-visible and subject
  to Linear event validation. Before any Linear write, Decodex must reject known
  leakage-shaped public projection text such as host-local paths, routed identity
  details, credential-like names, private account details, private config file
  names, emails, tokens, or secrets. Detailed checkpoint evidence remains
  local/operator-only.
- Dynamic tool names must satisfy the `codex app-server` identifier restriction `^[a-zA-Z0-9_-]+$`; dotted names are invalid.

## Failure handling

- If the agent never reaches a tracker write, `decodex` may perform a minimal fallback write during reconciliation or terminal failure handling.
- If a tracker tool call fails transiently, the failure should be surfaced to the run journal so retry logic can reason about it.
- If a tracker tool call fails because it targeted the wrong issue or an unsupported operation, treat that as a policy violation, not as a retryable transport error.
- When `[codex].review` is `"standard"` or `"strict"`, if the latest
  `issue_review_checkpoint` reports the same active accepted-finding fingerprint for
  the third time in the same phase, `decodex` must stop the current repair strategy
  and apply the loop-runtime architecture recovery boundary before any further
  autonomous repair. Different accepted-finding fingerprints start their own repeat
  counts, so a newly discovered issue does not inherit old churn. If the checkpoint
  reports `needs_architecture_review` / `blocked`,
  `decodex` must stop the lane through the human-required failure path instead of
  retrying automatically.
- Review-policy stops do not dispatch external investigation directly. `decodex` may
  surface operator guidance for a bounded follow-up, but any external workflow is
  valid only after a separate adapter contract can verify the current head, review
  phase, normalized stop kind, normalized error class, issue/run identity, and latest
  bounded-review evidence.
- If the turn completes without a valid recorded `issue_review_handoff` and without an explicit human-attention exit, `decodex` must treat the run as failed rather than silently moving the issue to `In Review`.
- If the turn completes without a matching `issue_terminal_finalize` call for the resolved terminal path, `decodex` must treat the run as failed before reporting the attempt as successful.
- If PR-backed success writeback partially succeeds, for example the issue reaches `In Review` but the completion comment fails to post, `decodex` must treat the lane as human-required and must not place it back on the automatic retry path.
- If a remaining public writeback validation failure occurs after successful PR
  validation, Decodex must classify it as `review_handoff_writeback_failed`, preserve
  the PR URL in the public recovery record when available, keep the already-written
  local review lifecycle row when terminal finalization succeeded, and stop in a
  recoverable human-required state instead of downgrading the completed
  implementation work to a generic coding failure.

## Future expansion

- A later phase may lift the transport from a dynamic tool bridge to a process-local MCP server if broader tracker or repo-collaboration tools are required.
- Any future expansion must preserve the issue-scoped safety boundary unless the user explicitly approves a broader trust model.

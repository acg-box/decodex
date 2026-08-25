---
type: "Reference"
title: "Runtime Lifecycle"
openwiki_generated: true
---

# Runtime Lifecycle

Scope: current v0.2 behavior only. For vNext target implementation, the
[vNext authority contract](vnext-authority.md) supersedes this page's Linear lane,
SQLite, Goal, transport, lifecycle, and authority model.

This page covers Decodex runtime authority, lane lifecycle, review lifecycle, app-server protocol, tracker tools, agent evidence, loop runtime, and autonomy boundaries. For the compact cross-cutting contract map, see [Runtime contracts](runtime-contracts.md).

## Authority model

Decodex coordinates coding-agent work through explicit project configs, workflow policy, local runtime state, Linear issue mirrors, and GitHub PRs. Source code, tests, project `WORKFLOW.md`, runtime SQLite records, and accepted Decision Contracts are authority. The global runtime database is the single-machine owner for leases, attempts, run-control channels, protocol summaries, private execution events, retry state, worktree mappings, and retained review lifecycle. Linear comments, GitHub PR metadata, logs, and marker files are collaboration or diagnostic projections unless the runtime explicitly records them as structured lifecycle evidence.

Important rule: do not reconstruct lifecycle truth from branch names, PR titles, Linear comments, or current `HEAD` alone. Recovery paths must use explicit evidence and persist a reviewed projection before normal dispatch resumes.

## Runtime and lane concepts

A lane is one retained unit of issue-scoped work. Its durable shape combines:

- a Linear issue identity
- a branch/worktree mapping
- a local lease
- one or more run attempts
- protocol summaries and private execution events
- review lifecycle records after PR handoff

The scheduler decides eligibility from queue labels, retry budget, retained post-review work, Program Intake nodes, and runtime recovery state. It must not run the same issue through multiple local active owners.

A normal attempt acquires the lease, prepares or reuses the deterministic linked worktree, starts one app-server session, records evidence under one `run_id` and `attempt_number`, and exits through exactly one continuation, retry, manual-attention, review-handoff, or retained post-review transition. Queued retry and continuation rows are runtime ownership claims until they fire, are cancelled, or become ineligible; they take priority over fresh candidate selection for the same issue.

## App-server protocol

Decodex starts Codex through `codex app-server --listen stdio://` for lane execution. The generated app-server schema and protocol tests are the source for request/notification shapes. Required capabilities include initialize, thread start/resume, turn start, dynamic tool calls, command exec health checks, thread archive, and phase-goal methods.

Phase goals are required for retained lane execution. If a Codex app-server build lacks the required protocol, Decodex should fail with a compatibility diagnostic rather than silently falling back to ordinary continuation. `decodex probe stdio://` should report `PROBE_OK` for a compatible runtime.

Protocol journals store ordered event identity and payload digests; compact summaries own startup and status readback. `thread/archive` and `thread/archive/discarded` are terminal barriers for one run, so later non-terminal app-server events are retained only as discarded recovery evidence and must not replace the terminal archive outcome.

## Lane control

Lane control is inspect-first. Steer and interrupt operations require current run identity and, for steer, the expected active turn id. Soft interrupt uses the app-server lane-control protocol when available. Forced interrupt may hard-kill only after the soft path is unavailable, rejected, or timed out under the runtime rules.

The control-plane state has four axes: `ownership_state`, `liveness_state`, `policy_state`, and `terminalization_state`. Running-lane counts and scheduler decisions come from ownership state, not from protocol activity alone. Liveness can report process/thread/protocol evidence, including late protocol activity, but it must not restore `leased_run` ownership after a terminal attempt, retired run control, or lost lease. Policy stops such as review churn, authority-boundary required, runtime recovery required, or human attention fence mutating tools until architecture recovery, runtime recovery, or explicit human action changes the policy state.

Owned-lane decisions use the action classes `continue`, `wait_for_external_signal`, `retry_automatically`, `resume_retained_lane`, `manual_intervention_required`, and `ready_to_land`. Contradictory tracker, retained worktree, PR, review, closeout, or cleanup evidence must choose `manual_intervention_required` instead of guessing. MCP lane control exposes the same model as a typed facade and does not bypass run/turn preconditions, capability profile, project enablement, issue ownership, or lane-control guards.

## Tracker tools and public ledger

The tracker bridge gives the child agent narrow issue-scoped tools: transition, comment, label add, progress checkpoint, review checkpoint, review handoff, review repair complete, closeout complete, and terminal finalize.

The bridge owns the public Linear execution ledger format. Public comments are structured projections and must not include credentials, auth material, local database paths, raw protocol payloads, hidden reasoning, private evidence bodies, or account identifiers. Private checkpoint details belong in runtime SQLite.

Tracker writeback is disposition-driven and private-first. Progress checkpoints persist the full normalized payload locally before any Linear projection. Review handoff requires the PR-backed handoff tool plus `issue_terminal_finalize(path = "review_handoff")`; manual attention requires the needs-attention label intent, a validated explanatory public summary, and `issue_terminal_finalize(path = "manual_attention")`. Terminal completion requires one valid terminal signal and explicit finalization, and Decodex should fail closed rather than infer completion from ordinary prose.

## Agent evidence

Agent evidence under `~/.codex/decodex/agent-evidence` supports diagnosis and handoff. Evidence can include handoff indexes, blocker snapshots, run capsules, protocol summaries, and private execution readback. It is not a public tracker record and should be summarized before any public projection.

Use `decodex diagnose` and `decodex evidence` for readback. Treat missing, stale, or mismatched run evidence as a recovery input, not as permission to guess lifecycle state. Startup and current-lane recovery may rebuild retained ownership only when deterministic worktree paths are paired with tracker/runtime/lifecycle evidence; stale PID markers, unscoped logs, and retained diffs alone are diagnostic, not authority.

## Review lifecycle

After PR handoff, lifecycle authority is represented by normalized review lifecycle records and append-only lifecycle events in the local runtime store. The pure lifecycle kernel classifies facts into `review_wait`, `review_repair`, `ready_to_land`, `landing`, `closeout`, `cleanup`, or manual-attention outcomes; adapters perform GitHub/Linear/local side effects. Missing or mismatched retained lifecycle records block automatic post-review dispatch; recovery must diagnose, adopt, or rebind from validated evidence instead of guessing from branch names, PR titles, Linear comments, or current `HEAD`.

Review rounds must read the current clean committed head, not memory of an older branch state. Each round requests review, receives it, validates and routes signals, repairs only accepted `current_blocker` findings, reruns required validation, then requests review again or stops. Decodex Review and GitHub Review rounds are counted independently; Decodex Review is the runtime-owned checkpoint loop, while GitHub Review is the strict `@codex review` adapter used only when the review level requires it. Missing acknowledgement resends are request retries, not new rounds. Outcomes are limited to clean, findings, needs architecture review, or blocked. Repeated active finding fingerprints after the convergence budget must stop the current patch strategy unless an Architecture Recovery Packet and Authority Boundary Check authorize a materially different strategy.

`issue_review_checkpoint` is the routing authority for review feedback. Accepted current repair work must be serialized through `finding_routes` as `current_blocker`; other routes such as landing blocker, authority decision required, needs evidence, follow-up, deterministic gate candidate, architecture signal, issue-contract gap, reviewer-rubric gap, risk note, or invalid/unsubstantiated remain durable evidence but must not silently become repair scope. Review-derived autonomy signals must consume this normalized current-head route evidence, not raw comments.

GitHub Review uses strict observable signals: the current PR head must already have authoritative green landing status evidence, the exact request comment must receive `eyes` from the `codex` actor, and a pass requires both the exact standalone text `Didn't find any major issues.` and a `thumbs-up` reaction on the PR description from that actor. If acknowledgement is absent after the configured resend budget, or if a review result arrives without the exact pass pair, the lane stops for manual intervention rather than inferring success. Repairs must validate findings against the current head; addressed threads may be resolved only after the fix is on the repaired head, verified there, and replied to in-thread, while pushback or clarification threads remain open.

Landing requires a non-draft PR, clear review blockers, acceptable clean merge state, configured landing statuses or legacy checks as applicable, an up-to-date branch, no unresolved authority-boundary landing block, and Decodex-owned `decodex land` closeout. Direct runtime merge is limited to the deterministic clean path; branch sync, conflict resolution, ambiguous mergeability, or repository-specific recovery must re-enter the retained agent path. Raw `gh pr merge` is not the Decodex-owned landing path. After a clean admin merge, Decodex waits only for bounded authoritative merge visibility; timeout, unsupported merge semantics, PR/branch lineage drift, or unresolved authority/architecture recovery outcomes require manual intervention. Once merge visibility is authoritative, retained closeout and deterministic cleanup are a short post-merge tail that reuses the merged PR evidence instead of requesting review or landing again.

## Loop runtime and Program Intake

The loop runtime is above individual issue lanes. Accepted Decision Contracts can be materialized through Program Intake into private Execution Programs and public issue briefs. Program dispatch is direct from runtime state, not queue-label polling.

Decision Contracts distinguish draft latent proposals, accepted promoted authority, human-decision blockers, and rejected superseded records. Only accepted authority can feed executable Program Intake. Accepted contracts preserve objectives, non-goals, constraints, assumptions, objections, stop conditions, validation expectations, risk notes, proposed issues, conflict domains, and acceptance provenance; acceptance is never inferred from project config, MCP auth, prompts, summaries, or local files. Program Intake dry-run is read-only, while apply may persist private Program Intake and Execution Program state only after accepted authority is present. Public issue briefs may summarize objectives, dependencies, validation, risks, and acceptance criteria; they must not leak internal graph ids, node ids, proposal ids, private evidence refs, or runtime row details.

Registered runtime policy config is a references-only binding, not caller-supplied or config-manufactured authority. Acceptance requires the interactive `decodex project accept-runtime-policy` ceremony, an OS-resolved local principal, exact Objective digest, RFC3339 timestamp within acceptance bounds, typed digest confirmation, and an atomically issued-and-consumed server-side receipt. MCP may preview the candidate but cannot perform acceptance, including through default Admin stdio.

Policy apply loads the immutable policy and exact Objective digest, takes the process-local plus project-keyed cross-process authority lock, runs a deterministic internal challenge bound to policy id/version, evaluator version, and the complete proposal including external challenge evidence but excluding its own self-referential internal rows, blocks all append-only recorded objections, and may promote only a ready accepted-Objective proposal. Compile, public challenge, legacy candidate creation, runtime promotion, and goal intake share the same lock. Same-id proposal replay preserves challenges. Existing contracts must match the complete candidate and deterministic promotion metadata, including acceptance time and reason. The existing `autonomy_request_promotion` contract remains latent-only. A trusted automation invokes policy apply explicitly; the daemon does not infer promotion authority by scanning proposals.

Policy apply does not invoke or retry Program Intake; `intake_goal` dry-run and apply remain separate authority boundaries. Decodex derives one canonical claim per contract and binds it to the exact project/config/workflow/team-anchor request digest. `prepared` is pre-mutation and retry-safe only for the same digest, `started` is externally uncertain and blocks automatic retry, and `completed` is terminal. Typed `decodex intake recover` inspection/reconciliation replaces raw database edits; `retry-prepared` performs the bound apply, and complete reconciliation requires exact contract-link, Program, plan id/kind/summary, node, mapping, issue, and fingerprint correspondence. Proposal objections appended after promotion are rechecked before intake. Missing policy or Objective authority, lineage drift, internal or recorded objections, non-ready queue intent, conflicting accepted records, rejected contracts, and partial/inconsistent/uncertain intake readback all stop closed. `absent` only permits a separate dry-run and first canonical apply.

## Autonomy control plane

Autonomy objectives, signals, and proposals are local planning surfaces. Objective Contracts are versioned project-level authority: drafts have no execution authority, accepted versions are immutable, and superseded/rejected versions remain provenance only. Signals are objective-version-bound evidence with source refs, freshness, contradictions, gaps, confidence, and privacy; they do not mutate tracker state, worktrees, GitHub, Program Intake, proposals, or execution state. Proposals are not executable by themselves and must preserve signal lineage, goals, surfaces, validation gates, review requirements, contradictions, gaps, rejected alternatives, rollback, and optional issue candidates.

Promotion requires explicit accepted authority. Accepting a proposal can create a normal latent Decision Contract candidate, but it does not create tracker issues, Program Intake rows, queue labels, worktrees, or execution lanes. Runtime-policy proposal acceptance requires a registered config binding, its matching immutable accepted runtime-policy record, the exact accepted Objective record, and a fresh internal challenge. External-agent output and caller-authored challenge evidence cannot manufacture or substitute that authority.

Autonomy must stay bounded to explicit allowed surfaces and signal kinds. Review-feedback signals must come from normalized review checkpoint routes for the current head, not raw comments. Stale signals need fresh readback, unresolved contradictions stay blocked, and review/validation weakening blocks promotion. Autonomy must not create hidden self-modifying authority, bypass review/landing gates, or mutate other projects without accepted project authority.

## Workflow policy

Project `WORKFLOW.md` owns tracker state names, labels, read-first paths, canonicalization commands, validation commands, and worktree hooks. Runtime code owns lease, run, retry, recovery, lifecycle, and closeout behavior. When workflow syntax changes, update parser tests and the operator documentation for command consequences.

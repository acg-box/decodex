---
type: "Reference"
title: "Contracts And Data"
openwiki_generated: true
---

# Contracts And Data

Scope: current v0.2 behavior only. For vNext target implementation, the
[vNext authority contract](vnext-authority.md) supersedes this page's Linear tracker,
SQLite, Goal, transport, data, and authority model.

This page is the primary map for Decodex contracts and data boundaries. Use it to identify the source file or test area that owns behavior before editing. For the concise runtime authority contract across state ownership, project/`WORKFLOW.md`, app-server, tracker writeback, privacy, and recovery, start with [Runtime contracts](runtime-contracts.md).

## Project contract

A registered project directory contains `project.toml` and `WORKFLOW.md`
(`apps/decodex/src/config/service.rs`). The project config parser denies unknown fields
(`apps/decodex/src/config/document.rs`). At freeze, `decodex.example.toml` modeled this
project config; the current checked-in file now owns the vNext global config shape.

Current `project.toml` shape:

```toml
service_id = "your-service-id"

[tracker]
api_key_env_var = "LINEAR_API_KEY"

[github]
token_env_var = "GITHUB_TOKEN"
landing_mode = "standard"

[codex]
review = "strict"

[paths]
repo_root = "/absolute/path/to/target/repo"
worktree_root = ".worktrees"
```

Optional frozen blocks include `[autonomy]`, `[autonomy.runtime_policy]`,
`[codex.accounts]`, and `[privacy_classifier]`. Config stores names of environment
variables and authority references; it does not embed live credentials, an Objective
body, or accepted policy authority. Interactive `decodex project accept-runtime-policy`
binds the OS-resolved principal, exact Objective digest, public non-goals, and current
RFC3339 timestamp to a typed digest confirmation, then issues and atomically consumes a
short-lived single-use receipt into one immutable record keyed by project, policy id,
and policy version. MCP may preview but cannot perform acceptance, including through
default Admin stdio. An exact accepted-record replay is idempotent and a conflicting
replay is refused.
`github.landing_mode` defaults to `standard`, which waits for GitHub's status
rollup. `fast` mode uses the stable `decodex/local-full-check` status and requires
`github.landing_actors` to name the trusted GitHub users or Apps that can publish and
execute fast landing.

## Workflow policy

`WORKFLOW.md` is project-owned execution policy. The runtime loads it through `WorkflowDocument` before each configured cycle (`apps/decodex/src/orchestrator/run_cycle.rs`). Source and old specs show these concerns are workflow-owned:

- tracker states and labels
- read-first files and prompting policy
- repo gate and validation commands
- canonicalization commands used by the baseline guard
- workspace hooks at worktree create/remove boundaries

Workflow policy constrains runtime behavior; it does not own runtime lifecycle authority. Worktree creation, leases, retries, post-review lifecycle, landing, closeout, cleanup, and recovery remain runtime-owned.

## Runtime database

The local SQLite database at `~/.codex/decodex/runtime.sqlite3` is the single-machine source of truth (`apps/decodex/src/runtime/paths.rs`, `openwiki/specs/contracts-and-data.md`). Schema bootstrap (`apps/decodex/src/state/sqlite_store/schema.rs`) creates:

- `projects`: registered project configs and enabled state.
- `leases`: one active local issue lease per issue.
- `run_attempts`: run id, project, issue, attempt, status, thread/turn ids, timestamps.
- `protocol_events`: keyed by `(run_id, sequence_number)` with payload SHA-256, not raw payload body.
- `protocol_event_summaries`: compact startup/status readback.
- `run_activity_summaries`: child-agent/protocol activity summary JSON.
- `worktrees`: issue-to-branch/worktree mappings.
- `linear_execution_events`: public tracker ledger event rows.

Bootstrap then installs schemas for worktrees, review lifecycle, evidence artifacts, run-control channels, connector backoff, private execution events, Decision Contracts, autonomy objectives/signals/proposals, Execution Programs, Program Intake state, and loop guardrails. Runtime-policy bootstrap upgrades pre-digest tables and revokes legacy accepted rows that cannot prove an exact Objective digest; the operator must accept the current policy again through the typed ceremony. These row families are authority records, not cache tables: Decision Contracts and Objective Contracts carry acceptance provenance, signals and proposals are private planning evidence, Program Intake records the dry-run/apply boundary, Execution Programs hold private dependency/conflict graph state, review lifecycle rows project retained post-review authority, and lane-control state gates mutating tools.

State ownership is local-first: the global runtime database lives outside registered repositories, scopes rows by `project_id`, and owns active leases, attempt status, retry state, run-control channels, protocol summaries, private execution evidence, worktree mappings, and retained PR lifecycle. Linear comments, GitHub PR metadata, logs, and `.decodex-run-activity` markers are mirrors or diagnostics unless a runtime adapter persists them as structured authority. Do not rebuild runtime truth from Linear comments on each tick.

## Leases, attempts, and run control

A Decodex lane is one issue, one branch/worktree mapping, and one or more run attempts. The runtime owns:

- issue lease acquisition/release
- dispatch slot and shared-claim checks
- attempt status transitions
- app-server child ownership
- run-control channel publish/retire
- retry budget and due-time scheduling
- recovery classification after crashes or stale children

Lane-control commands and MCP tools must use current run/turn authority instead of injecting commands into arbitrary threads (`apps/decodex/src/cli/control_commands/lane.rs`, `apps/decodex/src/mcp.rs`). Lane-control state has four independent axes: ownership, liveness, policy, and terminalization. Liveness evidence can make a lane diagnosable, but it must not recreate ownership after the lease/run authority is gone. Mutating tracker, review, closeout, worktree, or run-control tools must be fenced when terminalization is active or policy state requires architecture recovery, runtime recovery, or human attention.

The lease/attempt lifecycle is single-owner. A normal lane acquires the local lease, prepares or reuses the deterministic linked worktree, starts one app-server attempt, records protocol/private execution evidence under that `run_id` and `attempt_number`, then resolves exactly one continuation, retry, manual-attention, review-handoff, or retained post-review transition. Queued retry and continuation entries remain runtime claims until they fire, are cancelled, or become ineligible; they must take precedence over fresh queue selection for the same project/issue. Terminal-looking child process exit alone does not own the final state when persisted attempt or lifecycle records already reached a terminal success path.

## App-server protocol contract

`apps/decodex/src/agent/app_server/run.rs` is the source entrypoint for one attempt. The runtime contract is:

- Decodex starts `codex app-server --listen stdio://`.
- Generated app-server JSON schema is more authoritative than stale handwritten assumptions.
- `decodex probe stdio://` should pass with `PROBE_OK` for compatibility evidence.
- Required runtime methods include initialize, thread start/resume, turn start, thread archive, command exec health checks, dynamic tool calls, and phase-goal methods.
- Phase goals are mandatory for retained lane execution; Decodex rejects incompatible app-server builds instead of silently falling back to ordinary continuation.
- Protocol events are journaled with sequence and payload digest, while compact summaries own startup/status readback. `thread/archive` and `thread/archive/discarded` are terminal barriers for a run; later non-terminal app-server events are discarded local recovery evidence, not a reason to replace the terminal event.

When changing app-server behavior, inspect source, schema tests, and upstream Codex behavior when available. Do not tune liveness or terminal semantics from one local pilot run only.

## Tracker tool contract

The tracker bridge binds agent writes to the currently leased issue (`apps/decodex/src/agent/tracker_tool_bridge.rs`). Tool constants include:

- `issue_transition`
- `issue_comment`
- `issue_label_add`
- `issue_progress_checkpoint`
- `issue_review_checkpoint`
- `issue_review_handoff`
- `issue_review_repair_complete`
- `issue_closeout_complete`
- `issue_terminal_finalize`

Important boundaries from source and `openwiki/specs/contracts-and-data.md`:

- The coding agent may write only to the leased issue through supported operations.
- Generic arbitrary comments are not allowed; structured public summaries are rendered by Decodex.
- Private checkpoint payloads go to runtime SQLite before public Linear projection.
- Public projection text must omit raw local evidence, credentials, paths, account details, and hidden reasoning.
- Completion must produce exactly one valid terminal path and matching `issue_terminal_finalize`; Decodex must fail rather than infer intent.
- Review checkpoints are runtime-owned evidence writers for post-handoff/repair phases, not a generic agent escape hatch.

Tracker writeback is private-first and disposition-driven. `issue_progress_checkpoint` writes the full normalized checkpoint to private runtime events before any public projection; Linear receives only allowlisted public summaries. Successful implementation requires `issue_review_handoff` plus `issue_terminal_finalize(path = "review_handoff")`; manual attention requires the needs-attention label intent, a validated explanatory public summary, and `issue_terminal_finalize(path = "manual_attention")`. If signals are missing, mixed, or not explicitly finalized, the wrapper fails closed instead of guessing.

## Decision Contracts, Program Intake, and Execution Programs

The loop runtime sits above individual issue lanes (`openwiki/specs/contracts-and-data.md`, `apps/decodex/src/loop_contract.rs`, `apps/decodex/src/program_intake.rs`, `apps/decodex/src/execution_program.rs`).

Decision Contract statuses:

- `draft_latent`: candidate only, not executable.
- `accepted_promoted`: explicit accepted authority; may feed Program Intake.
- `needs_human_decision`: blocked until a human decides.
- `rejected_superseded`: must not become executable work.

Program Intake kinds:

- `goal_intake`: materialize an accepted Decision Contract.
- `issue_batch_intake`: materialize supplied existing Linear issues.
- Issue-batch Program ids derive from the service and normalized supplied identifiers, not mutable tracker state. Apply replaces the same batch in place and removes exact legacy duplicates after the replacement is durable.

Accepted Decision Contracts preserve objectives, non-goals, constraints, assumptions, objections, stop conditions, validation expectations, risk notes, structured proposed issues, conflict domains, and acceptance metadata. The runtime must not infer acceptance from a summary, prompt, local file, MCP auth profile, project config body, or caller-supplied policy object.

Program Intake has an explicit preview/commit boundary. Dry-run must not mutate Linear, Program Intake rows, Execution Program rows, issue mappings, or graph state. Apply may persist intake/program state only when the accepted authority already exists and the generated public briefs pass the privacy boundary.

Execution Programs are private runtime plans over normal issue-backed nodes. Nodes may be ready, held, blocked, running, completed, failed, or skipped according to dependency, conflict-domain, tracker, workflow, and lease state. Scheduler dispatches ready nodes directly with `program` dispatch mode; queue labels are not Program scheduling. Public issue briefs may describe objectives, dependencies, validation, risks, and acceptance criteria, but must not expose internal graph ids, node ids, proposal ids, private evidence paths, or runtime row details.

For issue-batch Programs, `active` is a snapshot of runtime ownership rather than a permanent operator pause. Reconciliation may return that intent to `ready_to_queue` only after live tracker, claim, retained-lane, and post-review ownership facts show the lane is no longer active. Other queue intents remain authority-bearing and are not rewritten by this recovery projection.

Objective Contracts are versioned project-level authority above Decision Contracts. Draft objectives have no execution authority; accepted versions are immutable; superseded and rejected versions remain provenance only. Signals are evidence bound to the exact accepted objective version and stable provenance; they cannot mutate tracker state, worktrees, GitHub, Program Intake, proposals, or execution state. Proposals bind signal clusters to goals, surfaces, validation gates, review requirements, contradictions, gaps, alternatives, rollback, and optional issue candidates. Accepting a proposal creates only a latent Decision Contract candidate unless a separate accepted Decision Contract or accepted project-policy authority promotes it.

The read-only MCP proposal resources include exact lookup by one public-safe affected identifier at `decodex://projects/{project_id}/autonomy/proposals/affected/{namespace}/{value}`. The runtime performs exact JSON-array membership lookup across all persisted proposals, refuses ambiguous duplicate matches, and returns the same redacted proposal summary contract as lookup by proposal id. This resource supports deterministic bridge replay and does not grant proposal acceptance, promotion, Program Intake, or execution authority.

Trusted runtime-policy promotion is resolved by Decodex, never supplied in an MCP request. Runtime-policy acceptance starts with the interactive `decodex project accept-runtime-policy` ceremony: Decodex resolves the OS principal, binds the exact accepted Objective digest and public non-goals, shows the candidate digest, requires the operator to type that digest, and atomically issues and consumes a single-use 10-minute receipt. Default Admin stdio cannot perform acceptance. Project deletion removes policies, pending receipts, and intake claims.

`autonomy_apply_runtime_policy` loads the immutable accepted policy record and exact accepted Objective digest, independently re-evaluates proposal lineage, all append-only recorded objections, evidence gaps and contradictions, allowed surfaces, validation/review/challenge gates, rollback, refusal reasons, and `ready_to_queue` issue candidates, then records reserved internal challenge provenance bound to policy id/version, evaluator version, and the complete proposal payload including external challenge evidence while excluding only Decodex's own self-referential internal challenge rows. A process-local mutex plus project-keyed cross-process file lock serializes compile, challenge, legacy candidate creation, runtime promotion, and goal intake. Same-id proposal replays preserve challenge evidence, and existing contracts must match the complete immutable policy-derived payload plus deterministic promotion provenance before replay is accepted. A successful apply promotes only the Decision Contract and reports Program Intake as `absent`, `partial`, `complete`, or `inconsistent`. `complete` requires exact contract links, one Program, one intake plan including plan id/kind/summary, all nodes, issue mappings, identifiers, and the accepted-contract fingerprint to correspond; orphan state under the deterministic Program id is inconsistent. Policy apply never supersedes another contract and never invokes Program Intake. A trusted automation or agent must call the typed MCP operation; `decodex serve` does not scan planning rows for implicit authority.

`intake_goal` remains a separate dry-run/apply boundary. Decodex derives one canonical claim per project and contract; callers cannot select or rotate idempotency keys. The claim also binds a digest of the exact project id, contract id, project config bytes, workflow bytes, and optional team issue anchor, so a prepared retry with changed inputs is refused. Current proposal objections are rechecked before dry-run or apply, including objections appended after promotion. `prepared` means pre-mutation checks may be retried, `started` means an external tracker mutation may have occurred and automatic retry is forbidden, and `completed` is terminal. `decodex intake recover ... inspect|retry-prepared|complete-after-readback` is the typed recovery route; `retry-prepared` executes the bound retry and completion reconciliation requires exact Program Intake readback. Partial, inconsistent, or uncertain intake must stop automatic retries and enter recovery.

## Review lifecycle records

After PR-backed handoff, retained review/landing/closeout lifecycle authority is the runtime DB `review_lifecycle_records` projection plus append-only private lifecycle events (`openwiki/specs/contracts-and-data.md`). Source areas include:

- `apps/decodex/src/orchestrator/kernel/post_review.rs`
- `apps/decodex/src/orchestrator/kernel/lifecycle.rs`
- `apps/decodex/src/orchestrator/retained_review_orchestration.rs`
- `apps/decodex/src/state/review_records/lifecycle.rs`

The pure lifecycle kernel decides from normalized facts; adapters perform side effects and persist projections. Linear comments, manual receipts, local branch names, PR titles, and current head heuristics are not final lifecycle authority. The persisted lifecycle authority projection records issue/project identity, phase, transition, previous/next state, next action, review gate state, PR URL, base/head branches, validated head, worktree path, merge/cleanup state, source evidence refs, idempotency/correlation ids, actor, and decision time. Historical handoff/orchestration tables and public ledger comments are not a substitute for this projection.

Fail-closed rule: if a retained lane lacks an exact lifecycle authority projection, normal/program/retry dispatch must not guess a lineage. Use explicit `decodex recover review-handoff diagnose`, `adopt`, or `rebind` paths depending on evidence.

Recovery boundaries are explicit. Startup/current-lane recovery may rebuild retained worktree mappings only from deterministic paths plus tracker/runtime/lifecycle evidence; missing lifecycle records, mismatched stored handoff heads, stale PID markers, and unscoped logs are diagnostic inputs, not ownership. Retained tracked changes after crash or stall flow through retry, phase-goal recovery, repo-gate recovery, or human-attention classification according to runtime evidence; they must not be rebound from branch names or PR titles.

## Commit message contract

`decodex commit` and `decodex land` use `decodex/commit/2` for tree-change subjects (`apps/decodex/src/cli/manual_commands.rs`, `openwiki/specs/contracts-and-data.md`). Canonical shape:

```json
{"schema":"decodex/commit/2","change":"short semantic summary","authority":"XY-123","impact":"compatible"}
```

Allowed authority values are a Linear issue identifier, `baseline`, or `manual`. Allowed impact values are `compatible` and `breaking`. Do not put PR URLs, branches, validation status, closeout state, cleanup state, CI status, retry state, or related issue lists into the commit subject. Those belong to lifecycle records, PR/tracker metadata, or local validation evidence.

## Privacy and public/private split

Public surfaces:

- Linear issue state, labels, and public lifecycle comments.
- GitHub PR status, review state, and commit statuses.
- Dashboard/MCP public-safe operator projections.
- Generated normal issue briefs.

Private/local surfaces:

- Runtime SQLite private execution events.
- Decision Contract and Program payloads.
- Autonomy signals/proposals.
- Run-control audit records.
- Protocol event details and summaries.
- Agent evidence under `~/.codex/decodex/agent-evidence`.
- Logs under `~/.codex/decodex/logs`.
- Account pool files and auth material.

If adding a field to a public projection, first identify whether it can expose local paths, credentials, raw logs, hidden reasoning, account identity, private evidence ids, or internal graph mechanics. If yes, keep it private or add a sanitized summary only.

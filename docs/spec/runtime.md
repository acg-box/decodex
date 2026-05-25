# Runtime Specification

Purpose: Define the authoritative runtime model for the `decodex` MVP.
Status: normative
Read this when: You need the authoritative model for issue eligibility, leases, lane ownership, runtime states, tracker-write ownership, or Linear writeback behavior.
Not this document: The low-level `app-server` protocol contract, the downstream `WORKFLOW.md` schema, or the operator pilot procedure.
Defines: The runtime scope, source-of-truth boundaries, eligibility rules, lane model, local state machine, tracker-write ownership, and writeback semantics.

## Scope

- One `decodex` service instance.
- One isolated linked Git worktree lane per eligible issue.
- One direct `codex app-server` session per run attempt.
- Supported host targets are Unix only: macOS and Linux. Windows is outside the runtime contract.

## Upstream alignment

- Upstream Symphony is the architectural reference for scheduler and runner ownership.
- `decodex` keeps two deliberate divergences:
  - Rust implementation instead of Elixir
  - TOML frontmatter in `WORKFLOW.md` instead of YAML
- `decodex` should align with upstream on tracker ownership: the coding agent should normally perform issue-scoped tracker writes autonomously through runtime tools, while the service remains responsible for leases, worktree lifecycle, retries, reconciliation, and crash-safe fallback behavior.
- Linked-worktree lane planning, creation, reuse, and cleanup are runtime-owned responsibilities. They must not depend on an installable workflow skill being present.
- Target-repository workspace hooks may run at linked-worktree create and remove boundaries when declared in the registered project `WORKFLOW.md`, but those hooks do not own lifecycle authority; the runtime still decides when a lane is created, reused, retained, or cleaned up.
- Installable Codex `AGENTS.md` guidance must not own Decodex runtime lifecycle,
  identity routing, tracker write policy, repo-gate commands, review, landing,
  closeout, or cleanup semantics. Those policies live in this spec, adjacent specs,
  centralized project contracts, or owning skills as defined by
  [`installable-agent-policy.md`](./installable-agent-policy.md).
- Current implementation note: normal-path tracker writes now flow through the issue-scoped tool bridge. Service-owned tracker writes remain only as fallback for reconciliation, crash recovery, and terminal failure handling.

## Source of truth boundaries

- The Decodex runtime SQLite database is the single-machine source of truth for active leases, attempts, protocol events, private execution events, worktree mappings, retained PR state, retry state, phase timing, project registration, tracker cache, PR cache, and connector backoff.
- Linear remains the team-visible tracker surface for issue lifecycle, queue/active/manual-attention labels, and coarse lifecycle summaries such as start, PR-ready, blocked, failed, landed, and done.
- Versioned Linear execution event comments use the schema in
  [`linear-execution-ledger.md`](./linear-execution-ledger.md), but fine-grained runtime truth must not be rebuilt from comments every tick.
- Private execution events are structured runtime evidence rows scoped by
  `project_id`, `issue_id`, `run_id`, and `attempt_number`. They hold full local
  evidence that should be queryable through `StateStore` without being mirrored to
  Linear execution ledger payloads. The operator CLI readback path is
  `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N>`, which reads the local
  runtime store and summarizes payloads by default.
- Centralized project directories under `~/.codex/decodex/projects/<service-id>/`
  form the project contract. Each directory contains `project.toml` for service
  paths and credentials plus `WORKFLOW.md` for execution policy. They do not store
  runtime ownership.
- The local SQLite database must not become a replacement for the human issue backlog. It is the operator control-plane state for this machine.

### Operator snapshot recovery boundary

Operator snapshots are local runtime views. They must remain useful when Linear is unavailable by reading the Decodex runtime SQLite database, retained worktrees, and locally cached connector state that already belong to this machine.

The following facts are local runtime truth and must not be rebuilt from Linear comments on every tick:

- lane attempts: `run_id`, `attempt_number`, attempt status, and terminal classification
- protocol events, event counts, event timestamps, and thread/liveness hydration fields
- private execution events carrying structured local evidence for an issue/run/attempt
- retry and backoff state: queued retry kind, due time, retry budget, and connector backoff
- phase timing and operator activity summaries
- retained worktree mappings, retained PR handoff identity, post-review phase, and cleanup or repair ownership

Linear issue fields and Linear execution ledger comments are the team-visible tracker mirror for low-frequency lifecycle records. They may enrich completed run history when the connector is available, but they must not become the live source for active leases, dispatch ownership, retry/backoff state, phase timing, retained worktree ownership, or operator snapshot continuity.

Operator snapshots must expose lightweight protocol event summaries, not materialized
event journals. Count and latest-event metadata such as `event_count`,
`last_event_type`, and `last_event_at` are dashboard data for liveness and progress
hydration; detailed protocol event history stays in the runtime database. This keeps
concurrent runs from amplifying snapshot size by copying full journals into every
operator-state refresh.

This boundary does not create a project-local runtime database contract. The runtime store remains the single-machine Decodex SQLite database under `~/.codex/decodex/`, scoped by `project_id`.

## Runtime tuning inputs

- Runtime policy decisions that depend on Codex behavior, such as idle timeout, stall thresholds, retry cutoffs, or liveness heuristics, must not be tuned from local Decodex observation alone.
- For those decisions, use three inputs together:
  - the generated `codex app-server` schema for protocol shape
  - live pilot telemetry for observed event cadence and failure modes
  - the relevant Codex or `app-server` implementation path for terminal semantics, waiting states, and progress signals
- If those inputs disagree, treat the local implementation and generated schema as more authoritative than stale design assumptions.
- Do not hardcode a wall-clock budget only because one pilot run happened to exceed or fit within it. Timeout and stall policy should be grounded in upstream runtime behavior first, then tightened with local evidence.

## Core terms

- Issue: One tracker work item visible to the service, usually admitted through the service-scoped `decodex:queued:<service-id>` Linear label derived from the registered project config `service_id`.
- Eligible issue: An issue that currently satisfies the `eligibility` rule in this specification.
- Lease: A local guarantee that only one active `decodex` run is processing a given issue.
- Run attempt: One bounded orchestration pass for one issue.
- Lane: The branch plus linked Git worktree checkout associated with one issue.
- Terminal tracker state: A state that should not be auto-started by `decodex`. The default set is `Done`, `Canceled`, and `Duplicate`.

## Eligibility

An issue is eligible only when all of the following are true:

1. The issue has the automatic intake label `decodex:queued:<service-id>` for the current service.
2. The issue state is in the configured `startable_states`.
3. The issue state is not in the configured terminal states.
4. The issue does not have the opt-out label `decodex:manual-only`.
5. The issue does not have the human-attention label `decodex:needs-attention`.
6. If the issue state is `Todo`, every blocker is already in a configured terminal state.
7. The issue does not already have an active `decodex` lease.
8. The project still has an available dispatch slot.
9. For generic normal dispatch, the Linear `description` surface still provides a generic issue briefing rather than only a machine-readable fenced block.

Typical configured `startable_states`:

- `Todo`

Optional future expansion:

- `Backlog`

`In Progress` should not be configured as startable in the normal case. `decodex` should not race human-owned work that is already in progress.

Current runtime note:

- Project-level concurrency must be explicit; set `[execution] max_concurrent_agents = 0` for no project-level cap, or use a positive integer for a finite cap.
- Active leases are the service-local claim set for running lanes, and shared dispatch-slot locks coordinate cross-process capacity when a finite cap is configured.

## Lane model

- One eligible issue maps to one branch and one linked Git worktree.
- One active run attempt owns the lane at a time.
- The lane path must be deterministic from issue identity so retries reuse the same checkout.
- The runtime owns lane planning, creation, reuse, and cleanup for those linked worktrees.
- The visible lane path lives under the configured worktree root, commonly `.worktrees/<ISSUE>` inside the target repository, while `git_dir` resolves under the repository's shared `.git/worktrees/*` admin area and `git_common_dir` resolves to the repository's primary `.git`.
- Before starting a live run, `decodex` must reject any prepared lane that is not a registered linked Git worktree for the configured repository.
- Worktree mappings and active leases must remain scoped to the registered project `service_id` so reconciliation does not cross project boundaries.

## Runtime state machine

The runtime state machine is local to `decodex`. It is not a replacement for Linear workflow states.

| State | Meaning | Exit conditions |
| --- | --- | --- |
| `discovered` | The issue was listed from Linear and passed the eligibility filter. | Acquire lease or skip on conflict. |
| `leased` | `decodex` created the local lease and reserved the issue for one attempt. | Worktree bootstrap starts or lease fails. |
| `worktree_ready` | The issue lane exists locally and is ready for execution. | `app-server` session starts. |
| `running` | `decodex` has an active `app-server` thread for the issue and may start one or more bounded turns on that thread. | A terminal completion path resolves, the bounded continuation budget is exhausted, the issue becomes non-active, transport fails, or policy violation occurs. |
| `validating` | Agent execution finished and the repo-native gate (`canonicalize_commands`, then `verify_commands`) is running. | The repo gate passes or fails. |
| `retry_wait` | The control plane is holding a queued retry entry for the leased lane after a clean continuation exit or a failure with remaining retry budget. | The queued retry revalidates and starts, the queued issue becomes non-active and the claim is released, or operator intervention cancels retries. |
| `needs_attention` | Retry budget is exhausted or human intervention is required. | Human updates the issue and it becomes eligible again. |
| `succeeded` | The attempt finished, validations passed, and the success writeback was committed to Linear. | Local cleanup begins. |
| `closed` | Local cleanup finished and the lease is gone. | None. |

After each `app-server` turn completes, `decodex` must resolve one continuation or completion outcome before deciding whether to start another turn on the same thread, enter `validating`, enter `needs_attention`, or yield to a retry path:

- `continue`
  - The turn ended without a terminal tracker path.
  - If the project-owned `execution.max_turns` budget still has room and the issue remains active for the leased lane, `decodex` starts another turn on the same thread and worktree.
  - If the issue is no longer active or the turn budget is exhausted, the worker exits cleanly and the control-plane continuation path decides whether to re-enter later or release the claim.

- `review_handoff`
  - The agent recorded a valid PR-backed review handoff and did not request human attention.
  - `decodex` proceeds into `validating`, then applies the success writeback if the repo gate passes.
- `manual_attention`
  - The agent explicitly requested human attention by adding `decodex:needs-attention` and did not also record review handoff.
  - `decodex` skips success writeback and the post-run repo gate, then enters the human-required failure flow immediately.
- invalid completion signaling
  - If the turn records both signals, or records one terminal path but fails to finalize it explicitly, the attempt is invalid and must fail rather than guessing a completion path.

## Tracker write ownership

- Preferred steady state: the coding agent writes tracker state transitions, comments, and handoff data for the currently leased issue through issue-scoped runtime tools.
- Service-owned tracker writes are reserved for:
  - startup reconciliation
  - crash recovery
  - terminal fallback when the agent never reached the point of writing the tracker
- The service must never grant the coding agent broad tracker write access outside the currently leased issue.
- `decodex` must treat the routed Linear `description` as a generic dispatch briefing surface, not as a plugin-private authority contract. If that surface contains only a machine-readable fenced block with no surrounding briefing text, generic normal dispatch is ineligible until another explicit briefing surface exists.
- Before starting a live run, the service must reconcile stale local leases and any terminal worktree mappings against current tracker state.
- Generic live dispatch must not require GitHub CLI authority before the lane actually attempts PR-backed review handoff.
- Generic live dispatch must resolve `github.token_env_var` before launching the agent app-server so lane-owned `git push` and `gh pr create` commands inherit noninteractive GitHub credentials. Missing or blank GitHub credentials must fail the run through the human-required path instead of retrying or leaving a promptable lane running.
- The service must fail fast on missing `gh` CLI authority only at the GitHub-dependent review boundary:
  - when a normal lane is about to validate and persist PR-backed review handoff
  - when a retained post-review lane is about to re-enter review repair
  - when a retained closeout lane is about to validate merged PR state or delete the
    retained remote branch ref

## Linear writeback model

### Start writeback

At the start of a normal run, the coding agent should:

1. Acquire the local lease.
2. Transition the issue to `In Progress`.
3. Post the applicable structured `run_started` comment.

The run-start comment is one Linear execution ledger record for new runs. It carries
the branch, repository-relative worktree path, current commit, transport, run id, and
attempt number instead of emitting separate intake, lease, worktree-preparation, and
agent-start comments. Its record envelope, event type, required fields, idempotency
key, and repository-relative `worktree_path` rules are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).

### Completion disposition

Before applying success or failure writeback, `decodex` must classify the finished turn into one and only one terminal completion disposition:

| Disposition | Required agent signal | Forbidden co-signal | Runtime effect |
| --- | --- | --- | --- |
| `review_handoff` | `issue_review_handoff` plus `issue_terminal_finalize(path = "review_handoff")` | `decodex:needs-attention` | Run the repo-native gate, revalidate PR state, post completion comment, transition to `In Review`. |
| `manual_attention` | `decodex:needs-attention` plus an explanatory issue comment, then `issue_terminal_finalize(path = "manual_attention")` | `issue_review_handoff` | Skip PR-backed success writeback and the repo-native gate, then treat the run as a human-required failure immediately. |

If neither signal exists, or both signals exist, `decodex` must fail the attempt instead of inferring operator intent.
If the label is recorded without the required explanatory comment, `decodex` must also fail the attempt instead of treating it as a valid `manual_attention` exit.
If the resolved terminal path is not explicitly finalized through `issue_terminal_finalize`, the app-server wrapper must fail the turn before `decodex` records the attempt as successful.
The explanatory comment for `manual_attention` must describe the exact observed blocker and should include the failed command plus raw error text when available instead of speculating about unverified capability limits.
Execution-state checkpoints are durable progress overlays only. Their phase, focus, next action, blockers, evidence, or verification fields are never a substitute for the explicit terminal-finalization call.

### Progress checkpoint writeback

`issue_progress_checkpoint` is private-first. Each accepted call appends the full
normalized checkpoint payload to `private_execution_events` in the runtime SQLite
database before attempting any Linear write. The private payload includes phase, focus,
next action, blockers, evidence, verification, resolved lane head, branch,
repository-relative worktree path, and PR URL when present.

Linear receives only the public projection of that checkpoint. The projection is a
`decodex.linear_execution_event` with `event_type = "progress_checkpoint"` and only
allowlisted public fields such as phase, summary, branch, repository-relative worktree
path, and PR URL. Raw checkpoint focus, next action, blockers, evidence, verification,
local head evidence, host-local paths, identity-routing details, account details, and
token names must stay out of Linear.

The local `linear_execution_events` table remains the public mirror cache for rendered
Linear records. It is not the private evidence source. Repeated checkpoint calls that
only change private payload fields must append private execution events but must not
append new Linear comments. A new Linear progress projection is written only when the
material public lifecycle signal changes.

When `decodex` runs the repo-native gate during `validating`, it must preserve the repo-gate failure class instead of collapsing everything into one generic failure bucket:

- `canonicalize_commands` non-zero exit: continued repair in the retained lane
- `verify_commands` non-zero exit: continued repair in the retained lane
- repo gate leaves tracked-file rewrites behind after its commands complete: continued repair in the retained lane
- repo-gate command spawn failures or tracked-file cleanliness inspection failures: human-attention failure path immediately

The continued-repair classes above are ordinary bounded churn: the coding agent should keep repairing code and rerun the repo gate rather than requesting `manual_attention` just because the gate has not passed yet. Human-attention exits remain reserved for environment, toolchain, or operator-owned blockers that the coding agent cannot clear from the retained worktree alone.

When `codex.internal_review_mode = "loop"`, handoff and retained review-repair runs also consume the latest structured `issue_review_checkpoint` state for the current phase and current lane head from the owned lane:

- no checkpoint and no terminal path: allow a clean continuation boundary
- latest checkpoint `clean` and no terminal path: allow continuation so the agent can finish handoff or repair completion
- latest checkpoint `findings` with fewer than three consecutive non-clean rounds in the same phase: allow continuation
- latest checkpoint `findings` with three or more consecutive non-clean rounds in the same phase: fail the turn through the human-required failure path
- latest checkpoint `needs_architecture_review` or `blocked`: fail the turn through the human-required failure path

`decodex` persists this review-policy state in the retained lane's `.decodex-run-activity` marker using `review_policy_phase`, `review_policy_status`, `review_policy_head_sha`, and `review_policy_nonclean_rounds`. Recording `issue_review_handoff` or `issue_review_repair_complete` clears those fields. When `codex.internal_review_mode = "prompt"` or `"off"`, Decodex does not expose `issue_review_checkpoint`, does not require a clean checkpoint before review handoff or repair completion, and ignores stale review-policy state while classifying clean turn boundaries.

The review-policy human-required failure path is also the boundary for any later
runtime-owned research escalation. The current runtime must not dispatch research from a
review stop. Future escalation may only consume structured review-stop evidence through
the adapter contract defined by [`review-orchestration.md`](./review-orchestration.md).

### Success writeback

This path applies only when the resolved completion disposition is `review_handoff`.

During the run, the coding agent should prepare a PR-backed handoff by:

1. pushing the lane branch
2. creating or updating a non-draft PR for that branch
3. calling the dedicated review handoff tool with the PR URL and a short summary
4. calling `issue_terminal_finalize(path = "review_handoff")`

After agent execution and post-run validation succeed, `decodex` should:

1. confirm that the recorded PR still belongs to the current repository and branch and that its head commit matches the validated lane HEAD
2. transition the issue to `In Review`
3. post the structured completion comment from the recorded handoff

If the `In Review` transition succeeds but the completion comment fails, `decodex` must stop automatic retries for that attempt and converge the lane through the human-required failure path instead of treating it as retryable work.

Structured review-handoff completion comments are `review_handoff` Linear execution
ledger records. Their required identity, PR, branch, commit, validation, summary, and
idempotency fields are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).

`In Review` is a PR-backed handoff state. Successful runs must not auto-transition directly to `Done`, and generic issue transitions must not move straight into the success state without the recorded PR handoff.

### Failure writeback

This path applies to retryable failures, retry exhaustion, and explicit `manual_attention` exits.

Retryable failures with remaining budget:

- Keep the issue in `In Progress`, typically through an agent-authored retry comment.
- Queue the retry in the runtime database rather than immediately redispatching inside the same poll tick.
- Clean worker exits after a nonterminal continuation boundary schedule a short continuation retry.
- Abnormal worker exits schedule exponential backoff capped by `execution.max_retry_backoff_ms`.
- When the queued issue disappears, reaches a terminal state, or otherwise becomes non-active before the retry fires, release the queued claim instead of redispatching it.

Terminal child-exit preservation:

- Failure retry scheduling is gated by the persisted run-attempt state, not by the final outer child-process status alone. If the persisted attempt is still active or has been recorded as failed when the child exits nonzero, the retry rules above apply.
- If the attempt has already persisted a successful terminal write, that completed run remains authoritative. A later nonzero outer child-process exit is diagnostic only and must not downgrade the attempt or enqueue a failure retry.

Retry-exhausted or human-required failures:

1. Transition the issue to `Todo`.
2. Add the label `decodex:needs-attention`.
3. Post a structured failure comment.
4. Finalize the terminal path with `issue_terminal_finalize(path = "manual_attention")`.

If the coding agent explicitly requests human attention by adding `decodex:needs-attention`, `decodex` must stop automatic retries for that attempt, skip PR-backed success writeback, and treat the lane as a human-required failure immediately.
Runtime-owned review-policy stops use the same human-required failure path, but with dedicated `error_class` values:

- `review_policy_exhausted`
- `architecture_review_required`
- `review_policy_blocked`

If the configured `decodex:needs-attention` label is unavailable on the team and the configured failure state is startable, `decodex` must still block automatic reselection by leaving the issue in a non-startable guard state such as `In Progress`. In that case the failure comment must explain that the label could not be applied and that a human must move the issue back to a startable state manually after repair. Restart recovery must preserve that guard by writing a retained-worktree marker under `.worktrees/<ISSUE>/.decodex-terminal-guarded` and consulting it before redispatching recovered `In Progress` lanes.

Any issue carrying `decodex:needs-attention` is ineligible for another automatic run until a human clears the label and returns the issue to a startable state.

Structured needs-attention and terminal-failure comments are Linear execution ledger
records. Their required identity, error, next-action, blocker, evidence, terminal-path,
and idempotency fields are defined by
[`linear-execution-ledger.md`](./linear-execution-ledger.md).

## Local operational state

The local runtime store is the global Decodex SQLite database for one local installation. It lives at `~/.codex/decodex/runtime.sqlite3`, not inside any registered project checkout or worktree. Every row that belongs to a repo is scoped by `project_id`. Decodex logs live beside that database under `~/.codex/decodex/logs/`, the optional shared Codex account pool lives at `~/.codex/decodex/accounts.jsonl`, global operator config lives at `~/.codex/decodex/config.toml`, bounded local account usage estimates live at `~/.codex/decodex/account-usage-history.jsonl`, and agent-readable derived evidence lives under `~/.codex/decodex/agent-evidence/<service-id>/`; vendor-qualified app-data directories and per-project runtime databases are not part of the runtime contract. Global operator config owns account-pool routing and shared account display-name offsets. Account usage history owns local seven-day display estimates only; it does not contain token material and does not decide scheduling. UI-only preferences such as theme, table sorting, and local privacy visibility are not runtime state.

Project contracts live outside registered repositories under `~/.codex/decodex/projects/<service-id>/`. Each project directory must contain `project.toml` and `WORKFLOW.md`; arbitrary project file names such as `<service-id>.toml` are not part of the contract. `project.toml` must set `[paths].repo_root` so the project contract is explicit. Project registration stores the centralized `config_path`, target `repo_root`, `worktree_root`, and workflow path in the global runtime database. Commands that start inside a registered checkout or lane worktree resolve the project through that registry; they do not discover or trust worktree-local config files. Project config refreshes preserve an existing enabled or disabled registry toggle; only explicit operator commands such as `decodex project add <project-dir>`, `decodex project enable <service-id>`, and `decodex project disable <service-id>` may change that toggle. `decodex serve` loads enabled registered projects from the global runtime database. It must not scan `.codex` history, repo-local config files, or currently open worktrees to infer additional projects.

The runtime database stores at least:

- registered projects and config fingerprints
- active leases and dispatch ownership
- run attempts and attempt status
- protocol event journals
- private execution events scoped by project, issue, run, and attempt
- worktree mappings
- retained PR and post-review state
- retry state and retry budgets
- phase timing and operator activity summaries
- tracker and PR cache rows needed to survive connector outages
- typed connector health and external API backoff

For child supervision, the active lane may also carry a short-lived worktree heartbeat marker at `.worktrees/<ISSUE>/.decodex-run-activity`. That marker is advisory, keyed to the current `run_id` plus `attempt_number`, and exists so the control plane can observe child activity across process boundaries, surface active thread and protocol progress in operator status, and keep high-frequency telemetry out of Linear. When the marker records process liveness, it must pair `process_id` with both host boot identity (`host_boot_id`) and process start identity (`process_start_identity`). A marker from a previous boot, a marker missing either identity, a marker whose process start identity no longer matches the live PID, or a marker observed while Decodex cannot read the current host or process identity must not be treated as a live process even if that PID currently exists. Operator snapshots expose `process_liveness_reason` so operators can distinguish stopped processes, previous-boot markers, and same-boot PID reuse from genuine live execution. The marker may also carry an additive `child_agent_activity` JSON summary for the current attempt; that summary is diagnostic state for operator snapshots, not durable scheduling authority. Operator snapshots must keep queue ownership separate from execution liveness: `active_lease` and `queue_lease_state` describe the local queue lease, while `execution_liveness` describes the observed process, app-server thread, or protocol marker that keeps an active lane visible. If a raw attempt is still `starting` after app-server thread, model, or protocol activity is observed, operator-facing `status` must report `running` and preserve the raw value in `attempt_status`. High-frequency heartbeat, child-agent buckets, token counts, idle ages, and other transient liveness details stay local/operator-only under the boundary defined by [`linear-execution-ledger.md`](./linear-execution-ledger.md).
Post-review ownership is stored in the runtime database. Retained handoff rows record the authoritative PR URL, branch lineage, validated PR head OID, run id, and attempt number that completed the `In Review` handoff. Retained orchestration rows record the current post-review phase for that exact handoff identity. If the matching database row is missing, post-review ownership must block as unresolved instead of rebinding from branch-name, current-head, Linear comments, or other heuristics. If a retained review marker exists but a stored handoff or orchestration head no longer matches a clean retained worktree and matching PR head, operator status must keep the marker PR URL visible when known and recovery diagnosis must report the concrete mismatched field before any explicit rebind refresh.
The only source-tree marker that clean-source checks may ignore is the untracked `.decodex-run-activity` heartbeat marker. Review handoff, orchestration, retry, phase timing, and retained PR state belong in the Decodex runtime database, not in root-level or worktree-local review marker files.

### Dispatch-slot handoff invariant

For live execution, project dispatch slots must remain mutually exclusive across concurrent `decodex` processes. The runtime may enforce that exclusion with short-lived worktree-root lock anchors, and control-plane parents may hand those guards to the spawned hidden `_attempt` child so the active lane keeps exclusive ownership even if the parent restarts. Because the runtime contract is Unix-only, that handoff may rely directly on Unix file-descriptor inheritance.

After the hidden `_attempt` child adopts the inherited issue-claim and dispatch-slot file descriptors (FDs):

- The child-owned dispatch-slot FD is the cross-process mutual-exclusion guard for the occupied slot. A competing `decodex` process must still observe that slot as unavailable while the child owns the descriptor.
- The parent must release its process-local issue-claim and dispatch-slot guard handles after the child adopts them. Any parent-side record left for observation or cleanup is bookkeeping only and must not hold an additional dispatch-slot FD or reserve another slot.
- The runtime database lease remains visible while the child owns the run. Releasing parent-local guard handles must not delete, hide, or downgrade the DB-backed active lease that operator status and restart recovery use to identify the running lane.

Restart recovery must use the runtime database plus retained worktrees and external caches instead of replaying Linear comments as the runtime ledger.

## Supported operator visibility surface

`decodex` must expose a supported local visibility surface for current runtime state without requiring operators to read source code or write ad hoc SQL.

The minimum supported surface is:

- structured runtime logs with stable identifiers such as `project_id`, `issue_id`, `issue`, `run_id`, `attempt`, `branch`, and repository-relative `worktree_path`
- a local status command that renders the current service snapshot in both human-readable and JSON forms
- an agent evidence command, `decodex diagnose`, that writes a compact derived handoff index, blocker snapshots, run capsules, and an append-only evidence event stream under `~/.codex/decodex/agent-evidence/<service-id>/`

Structured logs remain diagnostic. They may help explain a live failure, but they are
not the structured private evidence ledger. Private execution events belong in the
runtime SQLite store; Linear execution events remain the constrained public mirror for
coarse lifecycle records.

The status surface should describe runtime DB-backed execution state, plus low-frequency connector refreshes and retained `.worktrees` lanes, for example:

- active leased runs
- persisted run attempts with local status, thread id, and latest recorded protocol event
- registered project summaries with enabled state, fleet health/capacity counts, connector state, last activity, and retained worktree counts that exclude actively running lane worktrees
- queued tracker issues currently labeled for automatic dispatch, together with the current dispatch classification (`ready`, `claimed`, `blocked`, or `closed`) and any local policy reason that explains why they would or would not run next
- retained worktree mappings
- retained post-review lanes classified as `wait_for_review`, `needs_review_repair`, `ready_to_land`, `continue`, or `blocked`, together with the current PR/check metadata used for that classification

Retained worktree counts and recovery-worktree details must come from one consistent operator snapshot. If the summary count and detail list disagree, surface it as a snapshot consistency warning or bug, not as cleanup work for the operator.

After a process restart, recent-run history, active lease ownership, retained post-review state, and recovery worktree mappings must be reloaded from the runtime database before new work is scheduled. The control plane may refresh low-frequency tracker and PR cache rows, but it must continue publishing local operator state while Linear or GitHub is unavailable.

## Retention and cleanup

- Lease and session mappings: remove when the run closes.
- Attempt records, terminal outcome, private execution events, and locally cached
  Linear execution ledger links remain runtime history. Raw protocol event rows for
  terminal runs may be compacted by `decodex maintenance prune --apply` once the
  latest event is at least 14 days old, but only after Decodex writes the compact run
  summary and confirms that no active lease, retained worktree, review handoff, review
  orchestration, or cleanup blocker still owns that run or issue. The first private
  execution event schema has no compaction path; add one only when runtime maintenance
  owns a concrete retention policy for that structured evidence.
- `decodex maintenance prune --dry-run` is the read-only retention path for inspecting
  local cleanup candidates without applying retention changes. The `--apply` mode owns
  state-aware protocol-event
  compaction, old backup pruning, local log and agent-evidence event-stream rotation,
  and SQLite WAL checkpointing. Operators must not delete `runtime.sqlite3-wal`
  directly.
- `decodex serve` runs the auto-safe maintenance subset at startup and periodically
  while polling. That subset may rotate oversized local files, prune old backups, and
  run a passive WAL checkpoint, but it must not compact runtime protocol events.
- Worktrees: retain while the issue is non-terminal, and also retain terminal owned lanes while authoritative post-merge closeout or deterministic cleanup is still incomplete.
- Terminal issue cleanup: once the issue reaches a terminal tracker state and no authoritative post-merge tail remains pending, remove the worktree during reconciliation or startup cleanup.
- If an issue becomes non-terminal but no longer eligible while `decodex` is still preparing the lane, keep the worktree and skip execution for that pass.

## Recovery rules

- On service startup, `decodex` must inspect deterministic `.worktrees/<ISSUE>` paths together with tracker issue ids already known from local leases or worktree mappings to rebuild retained worktree mappings before starting new work.
- If Linear still shows a non-terminal `In Progress` issue and its retained worktree exists locally, `decodex` must treat that lane as a retry-style recovery candidate before selecting fresh `Todo` work.
- Retry recovery must bind retained lanes to issue identity and local runtime state rather than to Linear project membership.
- While the control plane is running an active lane, every poll tick must refresh cached tracker state for the leased issue before considering any new selection.
- While the control plane is running an active lane, that child must keep the workflow snapshot it started with; project `WORKFLOW.md` reloads affect later decisions without restarting the in-flight child.
- While the control plane is supervising an active child process, stall detection must consult the child-updated `.decodex-run-activity` marker for the current `run_id` plus `attempt_number` and the persisted runtime event journal. A retained marker only proves a live process when its PID is still alive on the current host boot and the process start identity still matches; after power loss, reboot, or same-boot PID reuse, recovery must clear the reconstructed lease and re-enter the retained lane through retry-style dispatch instead of preserving the old running state.
- Retry-style recovery prompts must tell the next agent to treat the current worktree, tracker state, protocol events, and marker files as durable truth, inspect the branch/diff/recent validation evidence first, and continue from partial work rather than assuming prior in-memory model/tool state survived.
- While the control plane owns a queued retry entry, that queued claim must take priority over normal candidate selection for the affected project.
- While the control plane is idle between lanes, it may reload the configured project `WORKFLOW.md` on each tick and immediately apply a newly valid document to future dispatch, retry, post-exit reconciliation, and prompt generation.
- If that same configured `WORKFLOW.md` path becomes invalid after a successful load, the control plane must log the reload failure and keep the last known good document active instead of dropping the tick or clearing runtime policy.
- If the leased issue becomes terminal during a control-plane tick, `decodex` must stop the active run, mark the attempt `terminated`, clear the lease, and then retain or clean the worktree according to the retention rules above.
- If the leased issue becomes non-terminal and leaves both the `In Progress` lane state and any configured startable pre-claim state, `decodex` must stop the active run, mark the attempt `interrupted`, clear the lease, and keep the worktree for inspection.
- If a recovered lease is already in `tracker.success_state` and its retained
  review-handoff marker matches the same `run_id` and `attempt_number`, reconciliation
  must mark the local attempt `succeeded` and clear only the lease so deterministic
  retained closeout can reuse the handoff identity.
- Deterministic retained closeout must take its `run_id` and `attempt_number` from
  the durable review-handoff marker or equivalent tracker record, not from a later
  same-process re-entry summary. Later local attempts that did not consume retry
  budget must not force a synthetic closeout attempt number.
- A leased issue that is still in a configured startable state during early control-plane ticks must be treated as a lane that has not finished claiming tracker ownership yet, not as an immediate non-active interruption.
- If a running attempt exceeds the app-server idle timeout with no recorded protocol activity, `decodex` must treat it as stalled, stop the active run, mark the attempt `stalled`, and converge the issue through the human-required failure path instead of silently retrying in this phase.
- If the supervised child already exited before the next control-plane tick, stalled reconciliation must still inspect the just-finished lane using recorded protocol activity rather than skipping directly to generic failure handling.
- Operator status snapshots must expose structured liveness and wait-state fields derived from persisted run markers, including current phase, optional wait reason, current operation, last run/protocol/progress times, idle age, a soft `suspected_stall` signal, and any queued retry kind plus due time, so operators can distinguish active execution from continuation waits, retry backoff, early stall suspicion, and genuine hard stalls without inferring progress from filesystem churn.
- Operator status snapshots may expose an additive `child_agent_activity` object when app-server protocol events have produced one for the current run. The object must stay machine-readable and dashboard/CLI shared, and should describe dynamic observed buckets rather than a fixed workflow: current child bucket and elapsed time, bucket wall/event/tool counts, current/max/cumulative input tokens, cumulative output tokens, largest tool output, and warnings for repeated large outputs. Missing `child_agent_activity` means no child breakdown was captured; existing JSON consumers must continue to work without it.
- If the agent Git credential preflight fails, operator status must report the retained lane as a credential failure requiring operator recovery, not as a still-running lane.
- If retry budget or needs-attention recovery finds tracked changes in the retained worktree, operator status must report retained partial progress rather than only a generic retry-budget hold. The failure class may be `partial_progress_retained` when no more specific runtime error class is available. Operators should then inspect the patch, finish validation and PR handoff if it is useful, or reset the retained worktree explicitly.
- During an active run, operator snapshots must expose `thread_id` as soon as the Codex thread exists, plus monotonically advancing `event_count`, `last_event_type`, and `last_event_at` once protocol events are recorded. These fields may be hydrated either from the current process journal or from the active lane's `.decodex-run-activity` marker when `status` is running in a separate process.
- `thread_id = null` is expected only before the worker creates the Codex thread for the current run. `event_count = 0`, `last_event_type = null`, and `last_event_at = null` are expected only before the first protocol event for that same run. After the thread exists and protocol activity has started, those empty values indicate missing hydration rather than normal progress.
- Operator snapshots may expose an additive `protocol_activity` object derived from app-server structured messages for the current run. The object stays local/operator-only and should summarize turn status, waiting reason, rate-limit status, and a compact recent event list for high-value app-server activity such as `turn/started`, `turn/completed`, plan updates, diff updates, item start/completion, command output deltas, server request responses, account updates, and rate-limit updates. Missing `protocol_activity` means no structured summary was captured yet; consumers must continue to rely on the older `event_count`, `last_event_type`, `last_event_at`, thread fields, and `child_agent_activity` fields when it is absent.
- The operator snapshot transport must stay local/operator-only. `decodex serve` exposes the human-facing operator console from the canonical HTTP `GET /` and `GET /dashboard` routes, serves only the necessary dashboard assets, `GET /livez` liveness probe, and local account-control API over HTTP, and delivers published snapshots, active-run activity, and dashboard control acknowledgements through the local `GET /dashboard/control` WebSocket upgrade.
- `GET /livez` is only a process- and listener-level liveness probe. It must not claim control-plane tick freshness or forward progress by itself.
- The dashboard must not depend on a separate HTTP snapshot or readiness endpoint; snapshot freshness belongs to the WebSocket-delivered snapshot payload and the browser connection state.
- Reconciliation must mark locally active run attempts as `interrupted` when their
  stale lease is cleared, `terminated` when the tracker issue is already terminal,
  or `succeeded` for the matching recovered review-handoff lease exception above.
- Failed, interrupted, or terminal-guarded retained repair or closeout attempts still
  consume retry budget and require later closeout dispatch to allocate the next
  attempt number instead of reusing the original handoff identity.
- Reconciliation must clear stale leases before the next issue-selection pass.
- When a queued retry becomes due, `decodex` must refresh that exact issue, redispatch it only if it is still active under retry policy, and otherwise release the queued claim.
- Before a prepared lane starts `app-server`, `decodex` must refresh the selected issue once more and skip execution if the issue became terminal or otherwise ineligible.
- After `app-server` initializes and before `thread/start` or `thread/resume`, `decodex`
  must run the bounded app-server capability preflight defined in
  [`app-server.md`](./app-server.md). Missing config/model/provider/skills/plugin/MCP
  state is a pre-dispatch terminal blocker with an operator-readable error class,
  not a promptable agent turn.
- If the local process crashed during a run, `decodex` must recover from the runtime database, current tracker cache or state, and retained worktree inspection.
- If Linear shows a non-terminal state but no local lease exists, the issue may become eligible again after reconciliation or may be redispatched through the retained recovered worktree.

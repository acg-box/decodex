---
type: "Spec"
title: "Workflow File Specification"
description: "Define the machine-readable contract for registered project `WORKFLOW.md` files consumed by `decodex`. Status: normative Read this when: You are authoring, parsing, or validating a registered project `WORKFLOW.md` file for use by `decodex`. Not this document: The `decodex` runtime state machine, the `app-server` protocol contract, or the operator pilot sequence. Defines: The file location, parse model, supported frontmatter structure, and the required `WORKFLOW.md` fields that `decodex` consumes."
status: active
authority: normative
owner: runtime
tags: [spec]
code_refs: [apps/decodex/src/workflow.rs, apps/decodex/src/orchestrator/git_ops.rs, apps/decodex/src/orchestrator/execution.rs]
drift_watch: [canonicalize_commands, verify_commands, repo_gate_lane_external_tracked_rewrite, repo_gate_tracked_rewrites_left, validation_evidence, phase_goal_next]
last_verified: 2026-06-23
---
# Workflow File Specification

Purpose: Define the machine-readable contract for registered project `WORKFLOW.md` files consumed by `decodex`.
Status: normative
Read this when: You are authoring, parsing, or validating a registered project `WORKFLOW.md` file for use by `decodex`.
Not this document: The `decodex` runtime state machine, the `app-server` protocol contract, or the operator pilot sequence.
Defines: The file location, parse model, supported frontmatter structure, and the required `WORKFLOW.md` fields that `decodex` consumes.

## File location

- Each registered project must store its active `WORKFLOW.md` as the centralized file
  colocated with that service's `project.toml` in the registered project directory:
  `~/.codex/decodex/projects/<service-id>/WORKFLOW.md`.
- Target repository roots are not active workflow-file locations. A repo-root
  `WORKFLOW.md`, if present, is not the runtime policy source for `decodex`;
  `project.toml` identifies the repository with `[paths].repo_root`.
- `decodex` may also target itself. In that mode,
  `~/.codex/decodex/projects/decodex/WORKFLOW.md` follows the same contract as any
  other registered project.

## Parse model

- `WORKFLOW.md` consists of TOML frontmatter followed by Markdown body text.
- The TOML frontmatter delimiter is `+++`.
- The Markdown body is the primary project-owned policy and prompt body that `decodex` injects into developer instructions.
- The frontmatter is the only machine-readable section of the file.
- The frontmatter is strict: every supported config field must be explicitly written, unknown fields are invalid, missing fields are invalid, removed fields are invalid, and `decodex` does not supply schema defaults.

## Control-plane reload semantics

- `decodex serve` may defensively reload the configured project-owned `WORKFLOW.md` on future poll ticks instead of relying on filesystem watchers.
- Edits to the centralized registered-project `WORKFLOW.md` policy should be
  followed by re-registering the project or restarting `decodex` before relying on
  the change, so the runtime project fingerprint and workflow snapshot are refreshed.
- When a reload of the currently configured `WORKFLOW.md` path succeeds, future dispatch, retry, reconciliation after child exit, and prompt generation may use the new document immediately.
- When that same configured path fails to parse after at least one successful load, the control plane must keep the last known good `WORKFLOW.md` active for future decisions and log a warning instead of dropping the whole tick.
- An already running child lane keeps the workflow snapshot it started with; reload semantics affect later decisions, not mid-run prompt or reconciliation behavior for that active child.

## Upstream divergences

- Upstream Symphony examples use YAML frontmatter. `decodex` intentionally uses TOML frontmatter instead.
- This divergence is deliberate and stable. Do not translate back to YAML only for stylistic upstream parity.
- Upstream Symphony treats the `WORKFLOW.md` body as the primary prompt and policy
  surface. `decodex` follows that model while storing the file in the central project
  directory instead of the target repository root.
- `decodex` also supports `[context].read_first` as a local extension for extra
  repository context files loaded from `[paths].repo_root`. This extra-context surface
  is not part of the upstream Symphony spec and must not replace the primary
  `WORKFLOW.md` body.

## Required top-level fields

- `version`

Current supported value:

- `1`

## Required tables

- `[tracker]`
- `[agent]`
- `[execution]`

Every top-level frontmatter table listed above must be present in v1.

## `[tracker]`

Purpose: Define tracker-facing policy.

Supported keys:

- `provider`
  - type: string
  - required
  - supported value for MVP: `"linear"`
- `startable_states`
  - type: array of string
  - required
- `terminal_states`
  - type: array of string
  - required
- `in_progress_state`
  - type: string
  - required
- `success_state`
  - type: string
  - required
  - note: `decodex` treats this as a PR-backed review handoff state, not a terminal completion state
- `completed_state`
  - type: string
  - required
  - note: successful post-merge closeout target; it must be a member of `terminal_states`
- `failure_state`
  - type: string
  - required
- `opt_out_label`
  - type: string
  - required
- `needs_attention_label`
  - type: string
  - required

Automatic intake is not configured in `WORKFLOW.md`. The runtime-owned automatic admission signal is the service-scoped Linear label `decodex:queued:<service-id>` derived from the registered project config `service_id`, while `WORKFLOW.md` only defines the repo policy that applies after an issue is already in that queue.

## `[agent]`

Purpose: Define registered-project defaults for the direct `app-server` session.

Supported keys:

- `transport`
  - type: string
  - required

Removed fields:

- `personality` and `service_tier` are not part of the v1 workflow contract.
- If they appear in frontmatter, `decodex` rejects the file as containing unknown fields.

Child-run execution policy is not part of the project-owned workflow contract. `decodex` must let `codex app-server` inherit sandbox and approval behavior from the active Codex runtime instead of declaring repo-local overrides in `WORKFLOW.md`.

Codex app-server-adjacent runtime settings such as `codex.review` belong to the
centralized project `project.toml`, not to `WORKFLOW.md`. `WORKFLOW.md` still owns
the bounded turn budget and repo gate used after a phase goal completes.

## `[execution]`

Purpose: Define target-repository execution and validation policy.

Supported keys:

- `max_attempts`
  - type: integer
  - required
- `max_turns`
  - type: integer
  - required
  - note: caps same-thread continuation turns inside one bounded run attempt; when set to `1`, Decodex preserves the current single-turn behavior
- `max_retry_backoff_ms`
  - type: integer
  - required
  - note: caps control-plane-owned failure retry backoff in milliseconds; clean continuation retries use a separate short fixed delay in runtime policy
- `canonicalize_commands`
  - type: array of string
  - required
  - note: use `[]` when there are no entries; every present entry must be non-empty and must not include surrounding whitespace
- `verify_commands`
  - type: array of string
  - required
  - note: use `[]` when there are no entries; every present entry must be non-empty and must not include surrounding whitespace
- `workspace_hooks`
  - type: table
  - required
- `gate_profiles`
  - type: table of named gate profiles
  - required
  - note: use `gate_profiles = {}` when there are no narrowed profiles

`canonicalize_commands` are the repo-native canonicalization gate surface. They may rewrite the worktree to bring it into repo-standard form before verification.

Before Decodex starts an ordinary issue lane, the runtime also uses the default
`[execution].canonicalize_commands` as the project baseline guard. The guard runs
those commands in an isolated clean worktree at the current remote default-branch
OID and requires no tracked diff. If canonicalization would rewrite the baseline,
Decodex does not lease or start the ordinary issue yet; it runs a Decodex-owned
baseline normalization path that commits exactly the canonicalization diff, opens
and lands a normalization PR, refreshes the default branch, reruns the baseline
guard, and only then resumes ordinary dispatch. This does not introduce another
workflow field or command runner; the default `canonicalize_commands` remain the
single authority for mutating canonicalization.

`verify_commands` are the repo-native read-only verification surface. They run after `canonicalize_commands` and must pass before review handoff, review repair completion, or landing-related push can proceed.

Together, `canonicalize_commands` and `verify_commands` are the default full repo-native gate. They run after agent execution and before the success writeback is committed, and they are also the required pre-push gate for PR-head refreshes, review handoff, review repair pushes, and landing-related sync unless a narrower named gate profile is selected. Local commits use the separate `decodex/commit/2` contract; they do not require any additional lifecycle-specific commit contract.

Removed execution fields:

- `max_concurrent_agents` is not part of the v1 workflow contract; Decodex does not apply a project-level concurrent-agent cap.
- `max_concurrent_agents_by_state` is not part of the v1 workflow contract.
- If either appears in frontmatter, `decodex` rejects the file as containing an unknown field.

### `[execution.workspace_hooks]`

Purpose: Define target-repository bootstrap and cleanup hooks around linked worktree lifecycle boundaries.

Supported keys:

- `after_create_commands`
  - type: array of string
  - required
  - note: use `[]` when there are no entries; every present entry must be non-empty and must not include surrounding whitespace
  - note: serial shell commands that run only after Decodex creates a brand-new linked worktree lane and before that lane is treated as ready for execution
- `before_remove_commands`
  - type: array of string
  - required
  - note: use `[]` when there are no entries; every present entry must be non-empty and must not include surrounding whitespace
  - note: serial shell commands that run before Decodex removes a linked worktree lane during runtime-owned cleanup
- `timeout_seconds`
  - type: integer
  - required
  - note: per-command timeout budget shared by both hook phases; values must be greater than zero

The v1 workspace-hook surface is intentionally narrow:

- only `after_create_commands` and `before_remove_commands` are supported
- `before_run` and `after_run` hooks are out of scope for this contract version
- commands run serially in declared order with the linked worktree root as the current working directory
- commands should be lightweight and idempotent

Runtime behavior:

- `after_create_commands` run only for a newly created linked worktree, not when Decodex reuses an existing lane
- if a newly created lane keeps a pending after-create marker because a previous `after_create_commands` run failed or was interrupted, Decodex must retry `after_create_commands` before allowing that retained lane to continue as a reused worktree
- `after_create_commands` fail closed; if a command fails, Decodex must stop before lane execution and keep the worktree for inspection
- `before_remove_commands` fail closed; if a command fails, Decodex must stop cleanup and keep the worktree instead of deleting it
- these hooks are repository bootstrap and cleanup policy, not a general-purpose orchestration plugin system

Stable environment variables exposed to workspace-hook commands:

- `DECODEX_REPO_ROOT`
- `DECODEX_WORKTREE_PATH`
- `DECODEX_ISSUE_ID`
  - note: the tracker issue identifier for the lane, for example `PUB-101`
- `DECODEX_BRANCH`

### `[execution.gate_profiles.<name>]`

Purpose: Define explicit narrow repository gate profiles that may replace the default full gate for clearly low-risk changed-file sets.

Supported keys:

- `match_mode`
  - type: string
  - supported value for the first slice: `"only"`
  - required
- `paths`
  - type: array of string
  - required
  - note: repo-relative glob patterns compiled by the runtime; every entry must be non-empty and valid
  - note: absolute paths, `.` components, and `..` components are invalid
- `canonicalize_commands`
  - type: array of string
  - required
  - note: every present entry must be non-empty and must not include surrounding whitespace
- `verify_commands`
  - type: array of string
  - required
  - note: every present entry must be non-empty and must not include surrounding whitespace

Each profile must declare at least one canonicalize or verify command.

`match_mode = "only"` means the profile applies only when every changed tracked file in the current lane is covered by the profile's `paths` set.

Selection semantics:

- if no profile matches, use the default full gate
- if exactly one profile matches, use that profile instead of the default full gate
- if multiple profiles match, use the default full gate
- if changed-file classification is unavailable or ambiguous, use the default full gate

The runtime must treat named gate profiles as a fail-closed narrowing mechanism, not as permission to skip required verification broadly. Path-based profile selection must not silently downgrade risky changes such as mixed docs-plus-code diffs, lockfile changes, or other unclassified paths.

The runtime-owned failure model for this repo gate must distinguish at least these classes:

- a `canonicalize_commands` entry failed
- a `verify_commands` entry failed
- a repo-gate command failed after writing tracked files outside the pre-gate lane
  diff, which is a scope-envelope violation requiring operator attention and
  structured source repo-gate evidence
- the repo gate completed its commands but left tracked-file rewrites behind,
  classified as lane-owned rewrites or lane-external tracked rewrites

The first two classes are explicit continued-repair outcomes in normal retained-lane policy: the retained lane stays in implementation or review-repair flow until the coding agent repairs the worktree and reruns the gate, or until retry policy is exhausted. The tracked-rewrite residue class remains available for lane-owned rewrites at strict lifecycle boundaries, while `repo_gate_lane_external_tracked_rewrite` identifies lane-external rewrites produced after a passing gate. During phase-goal validation only, if the repo gate commands pass and every rewritten tracked file was already present in the pre-gate implementation diff, Decodex may continue to the commit-capable handoff phase instead of terminalizing. If rewritten files are outside the pre-gate lane diff, Decodex records `lane_external_tracked_rewrite` evidence with file count, sample, and rewrite-set hash instead of scheduling another issue-local repair turn. Decodex must preserve the source repo-gate class and diagnostic as structured evidence for unsafe cases, but it must not add project-specific artifact, generated-file, fixture, or snapshot semantics to decide which tracked rewrites are acceptable.

Human-attention exits are reserved for repo-gate failures that the coding agent cannot reasonably repair from the worktree alone, such as command-spawn failures, missing runtime prerequisites, inability to inspect tracked-file cleanliness, ambiguous lane-external rewrites, or lane-external tracked rewrites that require explicit scoped authority. When the runtime takes that path, prompts and tracker comments should preserve the repo-gate source failure class instead of collapsing it into vague generic wording.

Landing policy is no longer repository-configurable in the machine-readable workflow contract for this repo surface. For retained review landing, `decodex` applies a fixed strict policy: require green configured landing status contexts bound to the current PR head and base tip, or legacy green checks when no landing contexts are configured, require an up-to-date base branch, preserve commit-level history, use merge commits, and never squash or rebase.

## `[context]`

Purpose: Define additional repository context files that `decodex` should load alongside the primary `WORKFLOW.md` body.

Supported keys:

- `read_first`
  - type: array of string
  - required
  - note: use `[]` when there are no entries; every present entry must be non-empty and must not include surrounding whitespace
  - note: entries must be normalized repository-relative file paths; absolute paths, `.` components, and `..` components are invalid

Paths are repository-relative.

## Forbidden content in frontmatter

The frontmatter must not include:

- machine-local absolute paths
- credentials or secrets
- host-specific worktree roots
- per-operator personal preferences that are not repository policy

Those values belong in `decodex` service configuration, not in `WORKFLOW.md`.

## Example

```md
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = [
  "cargo make fmt",
  "cargo make lint-fix",
]
verify_commands = [
  "cargo make check",
]
gate_profiles = {}

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
Use `cargo make` whenever an equivalent task exists.
Use the issue-scoped tracker tools autonomously when tracker updates are required.
```

## Body semantics

- The Markdown body is repository policy text.
- Issue-scoped developer instructions should include the `WORKFLOW.md` body first, then configured `context.read_first` files, then the explicit tracker tool contract.
- The body should contain durable repo rules, not ephemeral run notes.
- The body should instruct the coding agent to use the issue-scoped tracker tools autonomously when tracker writes are part of the repo workflow.
- Use `context.read_first = []` when the repository has no extra context files beyond the primary `WORKFLOW.md` body.
- Decodex must verify every configured `context.read_first` file exists and is readable before dispatch acquires a lane lease or records a run attempt. Prompt construction uses the same read path so stale paths report the project, workflow path, relative file path, and absolute file path instead of a generic filesystem error.
- If the repository expects PR-backed review handoff, the body should state that the lane must produce a reviewable PR before the success state can be reached.

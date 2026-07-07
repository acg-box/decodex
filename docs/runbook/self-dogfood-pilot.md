---
type: "Runbook"
title: "Self-Dogfood Pilot"
description: "Procedure for running a bounded Decodex self-dogfood pilot."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-06-16
---
# Self-Dogfood Pilot

Goal: Run the `decodex` MVP against one target repository and a bounded set of queued Linear issues, with `decodex` itself as the default first pilot target.
Read this when: You are preparing a dry run or live self-dogfood pilot and need the bounded operator procedure for config, target-repo requirements, and expected run behavior.
Preconditions: `codex app-server` is available locally; `gh` is available locally for live PR-backed handoff validation, merge inspection, and retained branch cleanup; the target repository exists on disk; the project contract exists under `~/.codex/decodex/projects/<service-id>/`; referenced `WORKFLOW.md [context.read_first]` files exist in `[paths].repo_root`; the Linear team exposes the required workflow states; and the tracker and GitHub token env-var names are configured through `tracker.api_key_env_var` and `github.token_env_var` in the centralized project config.
Depends on: `docs/spec/runtime.md`, `docs/spec/workflow-file.md`, `docs/spec/app-server.md`, the registered project `WORKFLOW.md`, and `Makefile.toml` for repo-native verification tasks.
Verification: `decodex probe`; `decodex project add ~/.codex/decodex/projects/decodex`; `decodex project list`; `decodex run --dry-run`; and, when the environment is ready, `decodex run`.

## Alignment note

- Normal-path tracker writes now belong to the coding agent through issue-scoped tools.
- `decodex` still owns startup reconciliation, local leases, worktree lifecycle, retries, and fallback tracker writes when a run never reaches the normal agent-owned path.
- Every live pass now starts with reconciliation of stale local leases and terminal worktree mappings before issue selection.

## Preconditions

- `codex app-server` is available locally.
- `gh` is available locally for live runs that must validate PR-backed review handoff.
- The host is macOS or Linux; Decodex does not support Windows.
- The target repository already exists on disk as a normal Git checkout.
- The registered project directory has a `WORKFLOW.md` beside `project.toml`.
- The target repository files referenced by `WORKFLOW.md [context.read_first]` exist under `[paths].repo_root`.
- The Linear team already has the workflow states used by the registered `WORKFLOW.md`.
- The Linear API token env-var name is configured through `tracker.api_key_env_var` in the centralized project config.
- GitHub auth for lane Git pushes, PR creation, review handoff, and post-review status is configured through `github.token_env_var` in the centralized project config; `decodex` does not fall back to ambient `GH_TOKEN` or an existing `gh auth login` session.

Recommended first-run check:

```sh
decodex probe
```

If `decodex probe` does not return `PROBE_OK`, stop there. The orchestrator loop depends on the same direct `app-server` contract.

## Recommended layout

For the recommended first deployment, collect each project contract under `~/.codex/decodex/projects/<service-id>/`, put service paths and credentials in `project.toml`, put execution policy in `WORKFLOW.md`, and set `[paths].repo_root` explicitly. If you need a redacted template for another checkout or workspace, start from `decodex.example.toml`.

```text
~/.codex/decodex/
  config.toml
  accounts.jsonl
  runtime.sqlite3
  logs/
  projects/
    decodex/
      project.toml
      WORKFLOW.md

/path/to/hack-ink/decodex/.worktrees/
  XY-123/
  XY-124/
```

`decodex` resolves config in this order:

1. `--config <PROJECT_DIR>` on project-scoped commands that accept it
2. the global project registry entry whose `repo_root` or `worktree_root` owns the current directory

Projects must be registered explicitly. Keep project configs in the canonical
directory `~/.codex/decodex/projects/<service-id>/`, make sure each `project.toml`
includes `[paths].repo_root`, then register and verify the entry:

```sh
decodex project add <project-dir>
decodex project list
```

`decodex serve` schedules enabled projects from that registry. It does not scan
`.codex` history, repo-local config files, or currently open worktrees to infer
projects.

Commands that refresh a project config keep the current enabled or disabled registry
toggle. Use `decodex project add <project-dir>` or `decodex project enable
<service-id>` when the intended action is to enable a project for scheduling, and use
`decodex project disable <service-id>` before a protected pause.

If the project uses managed ChatGPT accounts, enable `[codex.accounts]` in
`project.toml` and keep the JSONL pool at `~/.codex/decodex/accounts.jsonl`. Do not
store the shared pool under a project directory or configure a project-local account
path. To keep every new account-pool run on one account, set
`[codex.accounts].fixed_account = "<email-or-fingerprint>"` in
`~/.codex/decodex/config.toml`; the operator dashboard Accounts UI writes and clears
that same global selector. When the selector is absent, Decodex balances new runs
across the global pool.

After restarting `decodex serve`, verify the registry still points at the centralized
project directory:

- `decodex project list` should show the project config as
  `~/.codex/decodex/projects/<service-id>/project.toml`.
- `decodex status --json` or the operator UI should show no project backed by a flat `*.toml`
  config path inside a checkout or lane worktree.

Runtime state now lives in the Decodex-owned SQLite database at `~/.codex/decodex/runtime.sqlite3`, and logs live under `~/.codex/decodex/logs/`. On restart, `decodex` reloads retained worktree knowledge and current-lane recovery intent from that database, then refreshes low-frequency Linear and GitHub state as connector budgets allow.

That recovery is still scoped by configured `service_id`, so reconciliation and cleanup stay within the single service instance represented by the registered project config.

## Sample service config

```toml
service_id = "decodex"

[tracker]
api_key_env_var = "LINEAR_API_KEY"

[github]
token_env_var = "GITHUB_TOKEN"

[codex]
review = "basic"

# Optional secondary public-projection privacy guard.
# [privacy_classifier]
# endpoint   = "http://127.0.0.1:9123/classify"
# timeout_ms = 1000

[paths]
repo_root = "/path/to/hack-ink/decodex"
```

Notes:

- `service_id` scopes service-owned labels, reconciliation, and retained local state. Pick one stable service namespace per project config.
- `paths.repo_root` is required. Decodex does not derive it from the config file location.
- `paths.worktree_root` is optional. If omitted, Decodex uses `<repo_root>/.worktrees`. Relative worktree overrides are resolved from `repo_root`; relative `repo_root` overrides are resolved from the config file location.
- `transport` is defined in the registered project `WORKFLOW.md` and should normally be `stdio://`.
- Decodex does not expose repo-local model or reasoning overrides. `codex app-server` inherits those defaults from `~/.codex/config.toml`.
- `api_key_env_var` is required and must name the environment variable that stores the Linear API token.
- `github.token_env_var` is required for PR-backed review handoff validation and post-review PR-state inspection and must name the environment variable that stores the GitHub token.
- For the self-dogfood pilot, use `[codex].review = "basic"` when the lane should
  use only Self Check, `"standard"` when it should also require Decodex Review, and
  `"strict"` when it should additionally request GitHub Review after PR handoff.
  `"off"` skips review gates. If omitted, the default is `"strict"`.
- Retained lanes require phase-scoped app-server goals. If the selected Codex
  app-server lacks `thread/goal/*` methods, Decodex fails fast with an unsupported
  app-server blocker instead of using ordinary non-goal continuation.
- With non-strict review levels, a one-shot `decodex run` may continue draining the
  same retained lane after review handoff if the retained landing gates are already
  satisfied. If those gates are still pending, the run exits cleanly at the retained
  waiting boundary instead of spinning.
- `[privacy_classifier]` is optional and disabled when omitted. If enabled, `endpoint`
  must be an operator-managed loopback HTTP service; Decodex sends only rendered
  public projection text fields to it, never private runtime evidence.
- Automatic intake is driven by the service-scoped Linear label `decodex:queued:<service-id>` derived from the registered project config `service_id`. Keep the pilot bounded by applying that label only to the small issue set you want `decodex` to own.

## Target repository contract

The registered project directory must provide a parseable `WORKFLOW.md` with TOML frontmatter. For the MVP, the frontmatter contract lives in [`docs/spec/workflow-file.md`](../spec/workflow-file.md). For the first pilot, that means `~/.codex/decodex/projects/decodex/WORKFLOW.md`.

At minimum, the target repo should define:

- `[tracker] provider = "linear"`
- `[tracker] startable_states = ["Todo"]` or another explicit start set
- `[tracker] terminal_states = ["Done", "Canceled", "Duplicate"]` or another explicit terminal set
- `[tracker] in_progress_state = "<state name>"`
- `[tracker] success_state = "<state name>"`
- `[tracker] failure_state = "<state name>"`
- `[tracker] opt_out_label = "<label name>"`
- `[tracker] needs_attention_label = "<label name>"`
- `[agent] transport`
- `[execution] max_attempts`
- `[execution] max_turns`
- `[execution] max_retry_backoff_ms`
- optional `[context] read_first = [...]` only when the repo truly needs extra repo-local files loaded in addition to the `WORKFLOW.md` body; treat this as a Decodex-local extension, not as the primary policy surface

Child-run execution policy is not part of the project-owned `WORKFLOW.md` contract. `decodex` must let `codex app-server` inherit sandbox and approval behavior from the active Codex runtime instead of pinning repo-local overrides.

The target Linear team should also expose:

- startable states such as `Todo`
- handoff state such as `In Review`
- terminal states such as `Done`, `Canceled`, and `Duplicate`
- service-scoped label `decodex:queued:<service-id>` for automatic intake
- service-scoped label `decodex:active:<service-id>` for active lane ownership
- optional label `decodex:manual-only` to opt out of automation
- optional label `decodex:needs-attention` for retry-exhausted or human-required failures

If `decodex:needs-attention` does not exist, the run will still fail correctly. `decodex` will log a warning, explain the missing label in the failure comment, and keep the issue in a non-startable guard state instead of allowing another automatic retry from `Todo`.

## Recommended first scope

Use `decodex` itself as the first target repo and keep intake bounded by applying `decodex:queued:<service-id>` only to a small hand-picked issue set rather than a broad team backlog. That keeps the current dry run and live run inside one repo and one worktree root without coupling runtime identity to a Linear project.

## Running the pilot

### Dry run

Use dry run first to validate config loading, issue discovery, and worktree planning without mutating Linear or creating worktree directories.

```sh
decodex project add ~/.codex/decodex/projects/decodex
decodex project list
decodex run --dry-run
```

Expected behavior:

- loads the registered project `WORKFLOW.md`
- queries Linear for issues carrying `decodex:queued:<service-id>`
- applies the eligibility filter
- prints the selected issue, branch name, worktree path, and attempt number

If no config is found, the command exits cleanly with:

```text
dry run: no Decodex project config supplied or registered; nothing to execute.
```

### Live run

```sh
decodex run
```

On a normal successful run, `decodex` will:

1. reconcile stale leases and terminal worktree mappings for the configured service instance
2. select one eligible Linear issue
3. create or reuse a deterministic linked Git worktree
4. refresh the issue once more before execution and skip the lane if it became terminal or otherwise ineligible
5. acquire a local lease
6. let the coding agent perform the normal-path `In Progress` transition and start comment through issue-scoped tools
7. run Codex through direct `app-server`
8. run the configured repo-native gate inside the worktree (`canonicalize_commands`, then `verify_commands`)
9. require the coding agent to record a PR-backed review handoff and explicitly finalize the terminal path through the issue-scoped tool bridge
10. let `decodex` write the completion comment and `In Review` transition only after its own repo gate succeeds

An execution-state checkpoint alone is not a successful lane exit. Even if coding work and repository checks are done, the turn is still incomplete until the agent records either the review-handoff path or the manual-attention path and then calls `issue_terminal_finalize` for that same path.

### Signing verification checkpoint

Before treating a self-bootstrap run as healthy, verify the GitHub signing status at
both commit boundaries:

- After PR-backed handoff and before any auto-land or manual land, open the PR
  `Commits` view and confirm the current head commit shows GitHub `Verified`.
- If the PR head commit is unsigned, unverified, or signed by the wrong routed
  identity, stop landing. Rewrite the lane commit with the routed signing config,
  force-push the branch, rerun the required gate, and only then resume auto-land or
  manual land.
- After merge, open the merge commit on GitHub and confirm it also shows
  `Verified` before marking the run healthy or using it as proof for the next
  self-bootstrap batch.
- If the merge commit is not `Verified`, stop closeout and inspect the merge actor,
  merge method, and repository signing configuration before queueing more work.
- If manual land has already merged the PR and written tracker closeout, a later
  transient-label cleanup response of `Label not on issue` for
  `decodex:active:<service-id>`, `decodex:queued:<service-id>`, or the configured
  needs-attention label is an idempotent cleanup race, not a failed land. Missing
  active ownership before landing still blocks.

### Manual land cleanup verification

Use this only after an operator runs manual `decodex land` for a reviewed lane. For
retained-lane automation, use the `Review & Landing`, `Recovery Worktrees`, and `Run
Ledger` observations instead of treating this as a separate cleanup ceremony.

Set the landed lane values first:

```sh
ISSUE=XY-440
BRANCH=y/decodex-xy-440
PR=123
MERGE_SHA=$(gh pr view "$PR" --json mergeCommit --jq '.mergeCommit.oid')
```

Then check the cleanup tail:

- Verified merge commit:

  ```sh
  gh pr view "$PR" --json state --jq '.state'
  gh api "repos/hack-ink/decodex/commits/$MERGE_SHA" --jq '.commit.verification.verified'
  ```

  Expected: `MERGED` and `true`.
- Worktree removal: `git worktree list | rg "/\.worktrees/${ISSUE}\b"` should print
  nothing for the landed lane.
- Local branch removal: `git branch --list "$BRANCH"` should print nothing.
- Remote branch/prune verification: Decodex deletes the remote lane branch through the
  configured `gh` token. Run `git fetch --prune origin`, then
  `git branch -r --list "origin/$BRANCH"` and `git ls-remote --heads origin "$BRANCH"`
  should both print nothing.

If the manual land command already merged the PR and removed the landed lane, but then
failed because unrelated merged worktree cleanup debt remained, clean or salvage those
unrelated worktrees first. Then rerun the same explicit
`decodex land --manual-authority --pr <URL>` command from the repo-root default branch.
The recovery succeeds only when GitHub reports the PR as `MERGED`, local default branch
matches `origin/<default>`, the merge commit is already on that branch, the landed lane
branch/worktree is gone, and `Recovery Worktrees` has no merged cleanup debt. If the PR
is still unmerged, use the normal reviewed lane checkout instead.

After `probe`, `project add`, `run --dry-run`, and `run` all behave as expected, use `serve` for the long-running pilot loop:

```sh
decodex serve
```

### CLI observer loop

Use this path when the demo operator is starting from a clean `decodex` checkout and
wants to observe the self-bootstrap loop without reading source code.

1. Before queueing a self-bootstrap batch, confirm the active `decodex` CLI matches a
   checkout synced to the current landed `main`:

   ```sh
   git rev-parse --short HEAD
   decodex --version
   ```

   If `decodex --version` does not report the same short revision as the current
   landed `HEAD`, or after landing Decodex runtime changes, refresh the active CLI
   through the normal release path before starting the next batch. Stale CLI processes
   keep running pre-fix runtime behavior against new self-bootstrap issues, which can
   make dashboard or Linear evidence look like a new runtime regression.

   Before-batch stale-process check, after starting or restarting `decodex serve`:

   ```sh
   git rev-parse --short HEAD
   decodex --version
   SERVE_PID=$(lsof -tiTCP:8192 -sTCP:LISTEN)
   lsof -nP -iTCP:8192 -sTCP:LISTEN
   ps -p "$SERVE_PID" -o pid,lstart,command
   curl -fsS http://127.0.0.1:8192/livez
   ```

   Use the port from the active `--listen-address` if it is not `127.0.0.1:8192`.
   Treat a missing listener, a binary revision that does not match the current landed
   `HEAD`, or a `decodex serve` start time older than the latest runtime or dashboard
   landing as stale evidence. Restart `decodex serve` after refreshing the CLI or
   after any runtime/UI land, then rerun `/livez` on the same port and reload the
   dashboard before applying new queue labels. A browser tab left open on an old port
   is not evidence for the current serve process.

   Dashboard smoke checklist: after `decodex --version` matches the short `HEAD`,
   restart `decodex serve`. Verify `GET /livez` passes, then confirm
   `decodex status --json` and the browser dashboard agree on project registration and
   visible lane counts before queueing work.

2. Confirm the active CLI can reach the Codex app-server boundary:

   ```sh
   decodex probe
   ```

   If probe or a queued run reports `app_server_runtime_preflight_failed`,
   `app_server_introspection_method_failed`, `app_server_preflight_failed`, or an
   `initialize codexHome` mismatch, inspect the local `decodex serve` process
   environment and Codex runtime inventory before requeueing. The child app-server is
   pinned to the shared `$HOME/.codex` Codex home and state home; do not repair this
   by assigning a per-account `CODEX_HOME` or by overriding model, sandbox, approval,
   or personality settings from project policy.
   If the run reports retryable `app_server_plugin_list_timeout`, leave the issue in
   its active retry state and inspect the local `app_server_preflight_failed`
   evidence only if the retry budget exhausts or the timeout repeats. If status
   reports `attention_cause: app_server_plugin_list_timeout`, inspect the local
   evidence for the `plugin/list` timeout, restart `decodex serve` if the app-server
   process is stale, and run `decodex probe` until plugin inventory responds before
   clearing `decodex:needs-attention`.
   If preflight evidence shows `skills/list` enabled skills with scan diagnostics,
   keep the diagnostics as compatibility evidence and do not uninstall official skills
   solely to clear the scan error. Only missing cwd coverage or zero enabled skills are
   `skills/list` blockers; for those, inspect `first_error_path` and `first_error`,
   update the local Codex/Decodex compatibility or skill metadata as needed, restart
   `decodex serve`, and rerun `decodex probe` before clearing
   `decodex:needs-attention`.

4. In Linear, choose two or three small `decodex` issues for the demo batch. Keep each
   issue in a startable state such as `Todo`, make sure it does not carry
   `decodex:manual-only` or `decodex:needs-attention`, and apply the
   `decodex:queued:decodex` Linear label derived from the registered project config
   `service_id` only when the batch is ready to dispatch. After landing a runtime fix
   and restarting `decodex serve`, wait to apply the new queue labels until the restart
   observation checkpoint below is clean.

   Linear labels remain the team-visible intake surface. Runtime ownership lives in
   Decodex's local SQLite control-plane database. The control plane does not select
   work from GitHub labels, and GitHub labels are not used to queue, claim, opt out,
   retry, or recover a lane.

   Clean control-plane baseline before queueing:

   - The latest dashboard snapshot is fresh, and `Projects` shows the registered
     `decodex` connector as `ok`.
   - `Intake Queue`, `Running Lanes`, and `Review & Landing` show zero queued,
     running, or review lanes for the next batch.
   - `Recovery Worktrees` is empty for a clean baseline. A retained row is acceptable
     only when it is named as cleanup-only work, such as a landed or closed lane
     waiting deterministic worktree cleanup, and is not running, in review, or blocking
     active cleanup.
   - If any queued/running/review lane or unexplained retained worktree appears, pause
     before applying new `decodex:queued:decodex` labels and record the owner or cleanup
     reason first.

   Decodex-only 2-3 issue concurrent batch checklist:

   - Pick two or three `decodex` issues that are startable and small enough to review
     independently.
   - Apply the `decodex:queued:decodex` service-scoped intake label only to those
     Decodex issues. Concurrency is driven by eligible issues carrying that label, not
     by every enabled registered project.
   - Leave other registered projects, such as `rsnap`, enabled when they should stay
     visible in the pilot environment.
   - Do not add `decodex:queued:rsnap` when the intended test scope is Decodex-only.
     With no rsnap issues carrying that service-scoped queue label, the rsnap project
     stays visible but unqueued.

4. Register the project and start the control-plane loop from the repository root:

   ```sh
   decodex project add ~/.codex/decodex/projects/decodex
   decodex project list
   decodex serve --listen-address 127.0.0.1:8192
   ```

   `serve` owns one operator UI and schedules all enabled registered projects from the
   local runtime database. Passing `--config` refreshes that project registration
   before the scheduler starts.

   Do not use `decodex serve --dev` for this step. Dev mode is only for local
   account/app snapshot API development while avoiding scheduler activity; it does not
   register projects, poll Linear, dispatch work, or accept `--config`.

   Pass `decodex serve --config <PROJECT_DIR>` when you want `serve` to refresh one
   project registration before it starts. Omit it when the registry already contains
   the enabled projects you want the control plane to monitor.

   The scheduler keeps local snapshots on a 15-second loop and limits Linear-backed
   scans to one 5-minute window per project. After creating or relabeling queue
   issues, trigger the next scan explicitly instead of waiting for that window:

   ```sh
   curl -sS -X POST http://127.0.0.1:8192/api/linear-scan \
     -H 'Content-Type: application/json' \
     -d '{"projectId":"decodex"}'
   ```

5. Open the operator dashboard:

   ```text
   http://127.0.0.1:8192/
   ```

   For dashboard section meanings and local-vs-external state ownership, read
   [`../reference/operator-control-plane.md`](../reference/operator-control-plane.md).

   Restart observation checkpoint after a runtime fix:

   - Restart `decodex serve` from the same registered project setup and confirm
     `GET /livez` responds. Reload the dashboard and wait for a WebSocket snapshot; if
     it stays missing after one poll tick, stop before queueing new issues.
   - Check `decodex status --json` before applying new `decodex:queued:decodex`
     labels. The snapshot should show the intended `decodex` project registration, no
     previous completed lane counted as active work, and retained or recovery
     worktree entries only when they correspond to a live PR, review, landing, or
     recovery state.
   - In the dashboard, confirm `Projects`, `Intake Queue`, `Running Lanes`,
     `Review & Landing`, `Recovery Worktrees`, and `Run Ledger` agree with the same
     snapshot: the landed fix is visible through the latest run history, no stale
     retained worktree rows create noise, and no unexpected issue is waiting for
     an active claim to clear.
   - Queue the new Decodex-only issues only after those checks are clean.

   Post-land self-bootstrap observation checklist:

   - Reload the dashboard after the landed runtime fix is running again. The WebSocket
     snapshot should be fresh; if it stays stale after one poll tick, stop the demo
     loop.
   - Check `decodex status --json` before queueing follow-up work. Active and
     post-review counts should match the visible `Running Lanes` and `Review & Landing`
     rows.
   - Watch host CPU while the landed lane drains. CPU should return to the expected
     idle range after closeout instead of staying pinned with zero active work.
   - `Recovery Worktrees` should be empty except cleanup-only retained worktrees for
     landed or closed lanes waiting deterministic removal.
   - Stop before applying new queue labels if `decodex status --json` disagrees with
     the dashboard, active or post-review counts are inflated, CPU stays elevated, or
     retained rows lack a cleanup-only reason.

   Concurrent self-bootstrap observation checklist:

   - Before start, confirm `Projects` may show enabled projects such as `decodex` and
     `rsnap`, but `Intake Queue` contains only two or three queued `repo:decodex`
     issues for this Decodex-only batch. Do not use non-`repo:decodex` issues or add
     a queue label for unrelated projects such as `rsnap`.
   - During active work, confirm those issues appear concurrently in `Running Lanes`
     with separate issue ids and worktrees; stop if an unrelated project lane starts.
   - If the first attempt fails during `repo_gate`, leave the lane retryable and watch
     the next attempt. If repeated repo-gate failures cite setup drift such as
     `missing origin remote`, stop the batch and record the run id plus raw error in
     Linear before continuing.
   - To check restart recovery, restart `decodex serve` while at least one lane is
     active. After the next dashboard refresh, the existing active lanes should
     reappear in `Running Lanes` from runtime/worktree state. Any `ready` queued
     issues in `Intake Queue` should dispatch after the following tick; stop if
     recovered lanes are duplicated, lost, or replaced by
     new work.
   - During PR review and landing, confirm each lane reaches `Review & Landing` with
     a non-draft PR or leaves the active view only after retained closeout progresses.
   - Treat the concurrent restart test as clean only when queued Decodex issues move
     into `Running Lanes`, each lane keeps its own issue id
     and worktree, PR/review/landing state appears in `Review & Landing` and `Run
     Ledger`, and stale retained worktree entries do not reappear as active work or
     active-claim blockers.
   - After completion, confirm `Recovery Worktrees` is empty unless retained PR or
     recovery work exists, and `Run Ledger` shows every batch issue completed or
     explicitly needs attention.
   - After PR auto-land and Linear closeout, confirm each operator snapshot
     recent/history row shows completed/succeeded, not terminated.

6. Watch the dashboard in current UI terms:

   - `Projects`: registered projects such as `decodex` and `rsnap` can remain enabled;
     use this panel to confirm what the scheduler knows about without treating every
     enabled project as queued work.
   - `Intake Queue`: the queued Linear issues should appear as `ready`, `claimed
     without local lane`, or `blocked` before they start.
   - `Running Lanes`: the active issue should show its issue id, phase, current
     operation, attempt, health, timing, Codex thread details, and worktree path.
   - `Review & Landing`: after PR-backed handoff, retained lanes should move here with
     review, closeout, cleanup, waiting, blocked, or ready-to-land details.
   - `Recovery Worktrees`: this should usually stay empty during a clean demo; use it
     when retained PR lanes or orphaned local worktrees need inspection.
   - `Run Ledger`: completed issue lanes remain visible here from local runtime
     attempts, with Linear execution ledger comments used as the primary
     team-visible outcome when available. The primary row should show PR URL, merge
     or landed commit, closeout state, needs-attention reason, and elapsed lifecycle
     timing when the ledger recorded those fields; raw attempts and heartbeat details
     stay in the expanded debug view.
     To validate rich local closeout persistence after a lane completes, keep
     `decodex serve --listen-address 127.0.0.1:8192` running and inspect
     `decodex status --json` or the operator dashboard `Run Ledger`, not a replay of Linear
     comments. The completed row should still expose the local `run_id`, attempt
     number, lifecycle status, PR or landing reference, closeout or attention reason,
     elapsed timing, and debug-only raw attempt or heartbeat details after the lane
     has left `Running Lanes`.
     A clean review handoff followed by deterministic retained closeout should keep the
     same `run_id` and attempt number; a later attempt number should correspond to a
     real failed or interrupted retry.
     Use the post-XY-370 installed-binary docs-only canary, such as XY-371, to verify
     that this clean closeout path preserves the original handoff `run_id` with
     `attempt_number = 1` and does not add an interrupted attempt to status/dashboard
     history. If closeout reports `attempt-2` without a real failed or interrupted
     retry, record that as failed canary evidence. Record the PR URL, merge commit,
     and closeout identity in Linear before treating the canary as healthy.

7. Treat the loop as healthy when one issue moves from `Intake Queue` to
   `Running Lanes`, then either reaches `Review & Landing` with a non-draft PR or lands
   far enough for the lane to leave the active view. If a lane stalls or asks for human
   attention, stop queueing new demo work, inspect `Running Lanes`, `Recovery
   Worktrees`, the runtime DB-backed status view, the latest coarse Linear summaries,
   and the retained worktree named on the card.

### Local Evidence, Diagnostics, And Linear State

Use this boundary when the dashboard, retained worktrees, local evidence, logs, and
Linear issue state disagree.

Decodex stores private runtime evidence in one SQLite database owned by the local
Decodex installation:

- registered projects and config fingerprints
- run leases, dispatch slots, run attempts, retry schedules, protocol events, and
  local `Run Ledger` attempt rows
- private execution events for full checkpoint payloads, verification notes, local head
  evidence, and recovery details scoped by project, issue, run, and attempt
- linked Git worktrees under `.worktrees/<ISSUE>` plus shared Git administration under
  `.git/worktrees/*`
- current `status` and dashboard snapshots derived from the runtime DB, live process,
  retained worktrees, and low-frequency connector cache

Decodex writes derived local handoff evidence under
`~/.codex/decodex/agent-evidence/<service-id>/`. Use it to hand a repair agent a
compact `handoff-index.json`, blocker snapshots, run capsules, and a pointer to
`decodex evidence`. Do not treat those files as scheduling authority, GitHub/Linear
collaboration records, or a replacement for SQLite.

Logs and `.decodex-run-activity` are diagnostic. Logs explain process failures and
maintenance warnings. The activity marker explains live child process and protocol
liveness. Neither surface is the structured private evidence ledger, and neither should
be pasted into Linear as execution history.

Linear stores the public collaboration surface that teammates and later machines can
see:

- issue state such as `Todo`, `In Progress`, `In Review`, and terminal states
- Decodex control labels such as `decodex:queued:<service-id>`,
  `decodex:active:<service-id>`, `decodex:manual-only`, and
  `decodex:needs-attention`
- Linear execution ledger comments for low-frequency lifecycle records such as
  run-start, material progress phase, PR handoff, failure, landing, closeout, and
  cleanup summaries
- issue description, attachments, linked documents, human comments, and PR references
  that provide shared issue context

Do not treat Linear comments as the real-time runtime backend. Fine-grained timing,
retry state, raw attempt history, full checkpoint text, agent activity, connector
backoff, private evidence payloads, and recovery details belong in runtime SQLite or
local diagnostic surfaces. Sparse Linear history is healthy when the public lifecycle
summary is current and the full evidence can be read locally.

For recovery, start with the operator dashboard or `status`, then inspect private
evidence when the public summary is too terse:

```sh
decodex status --json
decodex evidence XY-123 --run-id <RUN_ID> --attempt <N> --json
```

Use `--include-payload` only for local repair. Do not paste full payloads into Linear or
GitHub. Inspect Linear state and comments after local readback when you need the
team-visible lifecycle record. A retained worktree or runtime DB recovery row means
"inspect this local lane"; it does not mean the team-visible issue state changed.
Removing a short-lived heartbeat marker does not erase the runtime DB row or the Linear
summary.

Decodex is intentionally Unix-only, and the control plane relies on Unix file-descriptor inheritance when the parent process hands the project dispatch-slot lock to the spawned hidden `_attempt` child.

`decodex serve` owns the local operator console. Use `--listen-address` when you need a non-default bind address:

```sh
decodex serve --listen-address <ADDR>
```

`serve` has no interval override. It publishes local operator snapshots every
15 seconds and runs Linear-backed queue/status scans at most every 5 minutes per
project. Use `POST /api/linear-scan` on the same listener to queue an immediate
scan request for the next 15-second tick.

Use hidden `decodex serve --dev` only for local account/app snapshot API development
while deliberately avoiding scheduler activity. Decodex App's fallback server uses
ordinary `decodex serve` and leaves scheduler cadence to CLI-owned defaults. Dev mode is
not a scheduler and must not be used for this runbook's automation, queue intake,
project registration, or retained-lane recovery steps.

The listener serves the operator console from the canonical `GET /` and `GET /dashboard` routes, the same JSON operator snapshot used by `decodex status --json` through the `/dashboard/control` WebSocket, and the minimal `GET /livez` liveness probe on the same listener. The single console keeps `Projects`, `Running Lanes`, `Intake Queue`, `Review & Landing`, `Recovery Worktrees`, and `Run Ledger` visible together. Intake candidates that are already claimed by a running lane are shown as claimed queue echoes, running lane worktrees stay with their owning lane, and retained/recovery worktrees remain folded until diagnostics are needed:

- `GET /` or `GET /dashboard`: the same single-page operator console
- `GET /dashboard/control`: WebSocket transport for snapshots, live run activity, and local dashboard control acknowledgements
- `GET /livez`: process-level liveness for the operator listener only

During `serve`, each poll tick now does two distinct things:

1. inspect any currently leased running lane
2. reconcile stale or terminal local state before selecting new work

The control plane also reloads the configured project `WORKFLOW.md` defensively on future ticks. A newly valid workflow document affects later dispatch, retry, post-exit reconciliation, and prompt generation without restarting the process. If the same configured path becomes invalid after a prior successful load, the control plane logs a warning and keeps the last known good workflow active; an already running child lane keeps the workflow snapshot it started with.

The running-lane reconciliation rules are:

- terminal issue: stop the lane, mark the run `terminated`, and remove the worktree
- non-terminal issue that has left both `In Progress` and any configured startable pre-claim state: stop the lane, mark the run `interrupted`, and keep the worktree
- issue still sitting in a startable state during early startup: leave it alone for that tick so the child can finish its initial tracker transition
- stalled lane with no app-server activity through the idle budget: stop the active attempt, mark the run `stalled`, and retry the same owned lane while retry budget remains; use the human-attention failure path only after retry exhaustion, retained tracked partial progress, or another terminal boundary
- child already exited before the next tick: still inspect persisted protocol activity so idle-timeout exits converge as `stalled`

## Worktree behavior

Each issue gets a deterministic lane:

- branch: `x/<project-id>-<issue-identifier>`
- path: `<worktree_root>/<ISSUE_IDENTIFIER>`

Example:

```text
branch  x/decodex-xy-123
path    /absolute/path/to/hack-ink/decodex/.worktrees/XY-123
```

Retries reuse the same worktree path.

If an issue becomes non-terminal but temporarily ineligible while the lane is being prepared, `decodex` skips execution for that pass and leaves the worktree in place for a later retry.

Each worktree is a linked Git worktree even though the visible directory lives under `.worktrees/<ISSUE>`. `decodex` attaches that lane through the source repository's shared `.git/worktrees/*` admin area, keeps `git_common_dir` anchored at the primary checkout's `.git`, and refuses to continue if the lane is not a registered linked worktree for that repository.

After running manual commands from a lane, check `decodex project list` or the operator snapshot if project paths look wrong. The registered project should still point at `~/.codex/decodex/projects/<service-id>/project.toml`; any config path inside a checkout or lane worktree should be replaced with the centralized project directory before restarting `serve`.

## Inspecting a failed run

Start with local runtime readback:

```sh
decodex status
decodex status --json
```

Use the human-readable view when you need the current leased run, lane worktree
ownership, and session-history summary at a glance. Use `--json` when you want stable
identifiers such as `run_id`, `issue_id`, `thread_id`, `branch`, and
repository-relative `worktree_path`.

If the status row or run capsule points to private evidence, inspect it before treating
the Linear summary as complete:

```sh
decodex evidence XY-123 --run-id <RUN_ID> --attempt <N> --json
```

Then inspect Linear for the public collaboration state:

- check the issue state
- read the latest Decodex ledger comment for public `run_id`, attempt number,
  timestamps, phase, and next action
- if retries were exhausted, look for the `decodex:needs-attention` label
- if the agent explicitly requested human attention, expect the issue to move back to
  `Todo` with `decodex:needs-attention` immediately instead of retrying
- any issue that still carries `decodex:needs-attention` is intentionally ineligible
  for another automatic run until a human clears that label
- if the failure comment says the label was unavailable on the team, expect the issue to
  remain in a non-startable guard state such as `In Progress` until a human moves it
  back to a startable state manually
- if the issue is already terminal, expect the worktree to disappear on the next live
  pass or startup reconciliation
- if the run failed as `stalled_run_detected`, expect the worktree to remain in place so
  you can inspect the partially completed lane before re-enabling automation

Parent repo-gate retry note:

- If a child already opened a non-draft PR and recorded review handoff, but the
  parent run later reports `repo_gate` failure, compare the child validation
  evidence, the parent gate command and error, the current PR head/check state,
  and the `run_id` plus attempt identity before treating the implementation as
  failed.
- A retry may update the same PR head or add a corrected follow-up commit on the
  retained branch. Treat the lane as healthy only after auto-land succeeds and
  Linear reaches `Done` with the service queue/active labels and
  `decodex:needs-attention` cleaned up.

Then inspect the worktree mentioned by status, private evidence, or the public ledger:

```sh
git -C /absolute/path/to/hack-ink/decodex/.worktrees/XY-123 status --short
git -C /absolute/path/to/hack-ink/decodex/.worktrees/XY-123 log --oneline --decorate -5
```

The operator snapshot also exposes coarse liveness semantics so you do not have to infer progress from worktree file churn alone:

- `phase = executing`: the lane is actively running
- `phase = waiting_continuation`: the worker ended cleanly at a turn boundary and Decodex may resume it
- `phase = retry_backoff`: the lane is not dead; Decodex has a queued retry and reports `retry_kind`, `wait_reason`, and `next_retry_at`
- `phase = stalled`: the lane crossed the app-server idle timeout and needs inspection

The snapshot also adds fields that make running lanes easier to interpret:

- `current_operation`: one of `idle`, `agent_run`, `repo_gate`, `review_writeback`, `waiting_external`, or `reconciliation`
- `last_protocol_activity_at`: the latest incoming app-server protocol event, including account, rate-limit, passive status, and other non-work traffic
- `last_progress_at`: the latest time Decodex recorded meaningful forward progress for the current lane; account, rate-limit, phase-goal, passive status, warning, model-routing, token-usage, and heartbeat-like events do not refresh it
- `suspected_stall = true`: a soft warning that progress has been quiet for a large fraction of the idle budget, before the lane crosses the hard `stalled` threshold
- `progress_diagnostic = protocol_only_activity`: the lane is still process/protocol-active in model execution, but recent protocol events are only non-work traffic and meaningful progress is stale or missing
- `child_agent_activity`: when present, a shared dashboard and `status` breakdown of dynamic child-thread buckets, context pressure, largest tool output, and repeated large-output warnings

When present, compare `current_operation`, `last_progress_at`, `last_run_activity_at`, `last_protocol_activity_at`, `progress_diagnostic`, `idle_for_seconds`, and `child_agent_activity.current_bucket` before assuming a lane is stuck. Quiet work with fresh child activity is different from a lane that is still alive but already drifting toward a stall; fresh account, phase-goal, or rate-limit events without fresh `last_progress_at` should be treated as protocol liveness, not proof of forward work.

For the running-lane fields:

- `thread_id` stays `null` only until the worker creates the Codex thread for the current run. Once that thread exists, `status` should show the live thread id even when the command is running in a fresh process.
- `event_count = 0`, `last_event_type = null`, and `last_event_at = null` are normal only before the first protocol event of the current run. After protocol traffic starts, those fields should advance monotonically from the running lane journal or its persisted worktree marker.

If you pass `--limit`, it only caps the recent-run section. Running lanes remain uncapped in both the human-readable and JSON status views so the currently leased lanes stay visible.

The runtime SQLite database is the supported recovery store, but operators should not debug it through ad hoc SQL first. If `status` and `decodex evidence` are insufficient, inspect the public tracker summary plus retained worktree lane directly:

1. Read the Linear issue state, labels, comments, attachments, and linked PR for `XY-123`.
2. Inspect the retained worktree:

   ```sh
   git -C /absolute/path/to/hack-ink/decodex/.worktrees/XY-123 status --short
   git -C /absolute/path/to/hack-ink/decodex/.worktrees/XY-123 log --oneline --decorate -5
   ```

Use the operator dashboard, `status`, and `decodex evidence` for run ids, attempts,
failure class, and private execution evidence. Use the retained worktree when the
failure happened inside `app-server` transport or thread lifecycle rather than during
repo gate commands. Linear should carry only the coarse team-visible failure summary.

## Re-running after failure

- If the run is still retryable, leave the issue in `In Progress` and let `decodex` retry.
- If the retryable failure class is `app_server_plugin_list_timeout`, treat it as a
  bounded app-server preflight timeout before `thread/start`: leave the active retry
  in place and inspect/restart the local serve process only if the timeout repeats or
  the retry budget exhausts.
- If `execution.max_turns` is greater than `1`, one bounded worker may now reuse the same app-server thread for multiple turns before it yields.
- Retryable control-plane retries now split into a short continuation retry after a clean nonterminal worker exit and a capped exponential failure backoff after an abnormal worker exit.
- If `status` reports `Git credential preflight failed`, configure the env-var named by `github.token_env_var` for the routed identity before clearing `decodex:needs-attention`; the lane never reached a promptable `git push`.
- If the failure summary reports `app_server_runtime_preflight_failed`,
  `app_server_introspection_method_failed`, `app_server_preflight_failed`, or an
  `initialize codexHome` mismatch, fix the local serve environment and Codex runtime
  inventory first. For runtime preflight blockers, check configured/default model,
  provider capabilities, enabled skills, plugin marketplace load errors, and MCP
  login state; for home mismatches, ensure `HOME` points at the user account that
  owns the shared `$HOME/.codex` tree. Restart `decodex serve` before clearing
  `decodex:needs-attention`.
- If status reports `attention_cause: app_server_plugin_list_timeout`, the workflow
  retry budget has exhausted for a bounded app-server preflight timeout before
  `thread/start`: inspect the retained worktree's local preflight evidence for
  `plugin/list`, restart `decodex serve` if stale, verify `decodex probe`, then clear
  `decodex:needs-attention` and move the issue back to a startable state only when
  another automated run is desired.
- If status or Linear terminal failure includes a `skills/list` preflight blocker,
  inspect the attached `first_error_path` and `first_error` details before changing
  local plugins or skills. Scan diagnostics with enabled skills are not blockers by
  themselves; missing cwd coverage or zero enabled skills require local
  Codex/Decodex compatibility repair before requeueing.
- If `status` reports retained partial progress or the dashboard shows `Partial patch held`, inspect the named worktree first. Treat the retained patch as local recovery evidence: finish the repo gate and PR handoff if the patch is useful, or reset the worktree before clearing `decodex:needs-attention`.
- If the run moved back to `Todo` with `decodex:needs-attention`, inspect the worktree, fix the blocking problem, clear `decodex:needs-attention`, and then move the issue back into a startable state for another automated attempt.
- If the issue should never be automated again, add `decodex:manual-only`.

## Verification commands

When changing `decodex` itself, keep the pilot path healthy with:

```sh
decodex probe
decodex project add ~/.codex/decodex/projects/decodex
decodex run --dry-run
cargo make fmt
cargo make lint-fix
cargo make check
```

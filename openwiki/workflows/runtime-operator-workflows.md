# Runtime Operator Workflows

This page is the operator-task map for Decodex. It describes what commands do, where they enter source, and what future agents should verify when changing them. For recovery and landing decision boundaries, see [Recovery And Landing](../operations/recovery-and-landing.md).

## Project setup and registry

Project configs live outside target checkouts under `~/.codex/decodex/projects/<service-id>/`. Each project directory contains:

- `project.toml` for service id, tracker/GitHub credential env-var names, optional Codex/autonomy/privacy settings, and repo/worktree paths.
- `WORKFLOW.md` for execution policy.

This is frozen v0.2 workflow provenance. The current `decodex.example.toml` now models
vNext global configuration, not this legacy project shape; inspect the trusted v0.2 tag
when auditing the historical template. Do not document or read live token values.

Commands (`apps/decodex/src/cli/control_commands/project.rs`):

```sh
decodex project add <PROJECT_DIR>
decodex project list
decodex project enable <SERVICE_ID>
decodex project disable <SERVICE_ID>
decodex project remove <SERVICE_ID>
```

`project add` registers or refreshes a config and enables it. Disable pauses future dispatch; it does not delete runtime visibility for existing DB-backed attempts. `decodex serve` schedules enabled projects from the explicit registry only; it does not infer projects from checkouts or open worktrees.

## One-shot execution

`decodex run` runs one orchestration pass (`apps/decodex/src/cli/control_commands/run.rs`, `apps/decodex/src/orchestrator/entrypoints/run.rs`):

```sh
decodex run --dry-run
decodex run --dry-run --explain
decodex run <ISSUE>
decodex run --config <PROJECT_DIR> <ISSUE>
```

Important behavior:

- `--dry-run --explain` is queue explanation only and rejects a preferred issue.
- Without a preferred issue, project selection may choose ordinary queued work, persisted Program dispatch nodes, retry candidates, retained closeout, or post-review work depending on runtime state.
- With an issue, dispatch mode is inferred unless a daemon child request supplies one internally.
- Retained post-review lanes that are waiting only for a missing runtime-owned
  standard review checkpoint are eligible as post-review repair continuations, so
  `decodex run <ISSUE>` can resume the retained lane instead of leaving the issue
  as "No eligible issue found".
- Tracker connector backoff is persisted and rendered instead of repeatedly failing the connector.
- Baseline canonicalization guard may run before normal/program/retry dispatch when workflow canonicalization commands exist (`apps/decodex/src/orchestrator/baseline_guard.rs`).

## Long-running serve and dashboard

`decodex serve` starts the local operator control plane (`apps/decodex/src/cli/control_commands/serve.rs`):

```sh
decodex serve --listen-address 127.0.0.1:8192
```

Core surfaces:

- `/` and `/dashboard`: dashboard UI.
- `/dashboard/control`: WebSocket dashboard/control stream.
- `GET /api/operator-snapshot`: published operator snapshot.
- `POST /api/linear-scan`: request a Linear scan for one project or all enabled projects.
- `GET /api/accounts?refresh=1`: account pool/API readback with optional fresh usage probes.
- `GET /api/lane/inspect`, `POST /api/lane/interrupt`, lane steer routes: local lane control.
- `GET /livez`: liveness.

`openwiki/workflows/runtime-operator-workflows.md` records current cadences: snapshots every 15 seconds; Linear-backed scans at most every 5 minutes per project unless explicitly requested. This frozen v0.2 operator control plane is outside the active workspace and is not bundled or started by the current Decodex App.

## Status, diagnose, and evidence

Status commands (`apps/decodex/src/cli/control_commands/status.rs`):

```sh
decodex status
decodex status --json
decodex status --live
decodex status --limit 25
```

Default status may reuse a recent default listener snapshot when it covers the requested project and limit. `--live` bypasses cache and refreshes tracker/PR observers before printing.

`decodex diagnose` writes and prints local agent-readable evidence. `decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N>` reads private runtime evidence from the local store. These outputs are repair aids, not scheduling authority (`openwiki/specs/contracts-and-data.md`).

## Orchestration kernel lifecycle cutover

When changing runtime lane policy, keep the cutover boundary locked: Self Check and Basic review behavior, agent-facing review checkpoint tools, legacy marker compatibility, and dual old/new lifecycle branches must stay out of active runtime. Standard review is runtime-owned; Strict review and GitHub review semantics remain fail-closed; current-head clean checkpoint, PR/head/worktree lineage, and dirty-worktree review blockers remain required.

The kernel vocabulary is the operational contract. `OwnedLaneAction` remains the domain action set (`continue`, `wait_for_external_signal`, `retry_automatically`, `resume_retained_lane`, `manual_intervention_required`, `ready_to_land`), while side effects are `CommandIntentKind` values such as `start_retained_landing`, `start_review_repair`, `finish_retained_cleanup`, and `sync_review_lifecycle_authority` with idempotency keys, preconditions, and expected postconditions (`apps/decodex/src/orchestrator/kernel/action.rs`, `apps/decodex/src/orchestrator/kernel/command.rs`).

For post-review work, the lifecycle authority record is the source of truth after review handoff. Mutations must flow through lifecycle decisions and `StateStore::record_lifecycle_decision`, which writes the authority projection and append-only lifecycle event together; do not infer post-review truth from branch names, PR titles, current HEAD alone, Linear comments, status rows, dashboard labels, or old marker-shaped names (`apps/decodex/src/orchestrator/kernel/lifecycle.rs`, `apps/decodex/src/state/review_records/lifecycle/authority.rs`). Status, dashboard, MCP, recovery, landing, and closeout code are projections or side-effect adapters over that authority, not independent lifecycle policy owners.

Before calling the cutover complete, run reverse scans for removed review checkpoint surfaces, Self Check/Basic review language, and old marker authority names across `apps/decodex/src`, `docs`, and `plugins`, excluding historical runbook/log text when appropriate. Validation must include focused lifecycle/post-review/kernel tests, broad Rust validation or documented unrelated-failure evidence, `git diff --check`, and a final architecture review with no unresolved blocker. Completion means old lifecycle decision branches are deleted or reduced to adapters, every mutating path uses kernel command intents, and every read surface consumes projections.

## Lane control

Lane commands (`apps/decodex/src/cli/control_commands/lane.rs`):

```sh
decodex lane inspect <ISSUE> --run-id <RUN_ID> --json
decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>
decodex lane interrupt <ISSUE> --run-id <RUN_ID> --reason <TEXT>
decodex lane interrupt <ISSUE> --run-id <RUN_ID> --force
```

Rules:

- Inspect first when possible. Steer requires the current run id and expected active turn id.
- Soft interrupt uses app-server lane-control protocol when available.
- `--force` allows hard process-kill fallback only after soft interrupt is unavailable, rejected, or times out under the documented conditions.
- If soft control reports `run_lease_missing` while inspect/status still shows the same run id, attempt, branch, active channel, and live process or protocol activity, treat the lane as degraded active execution; do not classify it as cleanup-only or clear attention labels.
- Lane-control results and audit records are local runtime evidence; they do not replace tracker lifecycle signals.

After lane control, choose the next recovery path from evidence: wait for a matching live lane to settle, resume only when retained work and PR lineage exactly match, run stale-active recovery for tracker-present active-label claims without safe live/progress ownership, run ghost-lane recovery for missing-issue local state without retained evidence, run review-handoff recovery for PR-backed lifecycle drift, or stop for manual attention when evidence is missing or contradictory.

MCP `decodex_lane_control` mirrors these preconditions as an inspect-first facade, not a shortcut.

## Program Intake

Program Intake turns accepted planning into executable issue-backed runtime state (`apps/decodex/src/cli/research_intake_commands/intake.rs`, `openwiki/specs/contracts-and-data.md`):

```sh
decodex intake goal --project <SERVICE_ID> <CONTRACT_ID> --dry-run
decodex intake goal --project <SERVICE_ID> <CONTRACT_ID> --apply
decodex intake issues --project <SERVICE_ID> <ISSUE>... --dry-run
decodex intake issues --project <SERVICE_ID> <ISSUE>... --apply
decodex intake recover --project <SERVICE_ID> <CONTRACT_ID> inspect
decodex intake recover --project <SERVICE_ID> <CONTRACT_ID> retry-prepared
decodex intake recover --project <SERVICE_ID> <CONTRACT_ID> complete-after-readback
```

Rules:

- Goal intake requires an accepted Decision Contract. Draft, rejected, or needs-human-decision contracts are not executable.
- Dry-run reads and renders a deterministic report without mutating Linear or runtime Program Intake rows.
- Apply may create/update generated normal Linear issue briefs for goal intake and persist local Program Intake/Execution Program rows.
- Goal apply uses one server-derived canonical claim per contract, bound to the exact project/config/workflow/team-anchor digest. `prepared` may retry only with those same inputs; `started` must not retry automatically because a tracker write may have occurred; `completed` is terminal. Newly recorded proposal objections block intake even after promotion.
- Recovery inspection is read-only. `retry-prepared` performs the bound apply and accepts `--team-issue` only when it matches the original digest. `complete-after-readback` succeeds only when a started claim has exact contract-link, Program, plan id/kind/summary, node, mapping, issue, and fingerprint correspondence.
- Issue-batch apply persists local Program Intake/Execution Program state for existing issues.
- Issue-batch Program identity is stable for the service and normalized supplied issue identifiers. Reapplying the same batch refreshes its tracker snapshot in place and retires exact legacy duplicates rather than accumulating competing Programs.
- Runtime reconciliation releases an issue-batch node whose persisted `active` intent came from ownership that is now absent, while preserving explicit terminal, paused, and not-ready intents.
- A persisted `continuation_pending` run remains eligible for daemon continuation even when the child process exits non-zero after the continuation boundary. The persisted phase cursor and lane policy remain authoritative; the exit code alone must not clear its retry schedule.
- Program dispatch is direct. It does not apply, remove, or wait for service queue labels.
- Internal graph/node ids, proposal ids, private evidence refs, and local runtime rows must not be exposed in public Linear briefs.

## Recovery workflows

Recovery commands live under `decodex recover` (`apps/decodex/src/cli/recovery_commands.rs`):

```sh
decodex recover review-handoff ...
decodex recover ghost-lane ...
decodex recover stale-active ...
decodex recover legacy-closeout ...
decodex recover merged-closeout ...
```

Use recovery when normal runtime status reports retained lane drift, missing lifecycle authority, ghost lanes, stale active ownership, or already-merged closeout gaps. The post-review lifecycle spec is strict: missing lifecycle records are fail-closed and must not be reconstructed from branch names, PR titles, Linear comments, or current head alone (`openwiki/specs/contracts-and-data.md`).

Recovery is dry-run-first unless the command is read-only. `review-handoff diagnose` is the read-only entrypoint; `rebind` repairs retained Decodex PR lanes only when the retained worktree and PR prove exact issue/branch/head authority, while `adopt` is limited to human-created PRs from a managed clean Decodex worktree and must not take over lanes that already have lifecycle records. `stale-active diagnose` applies to tracker-present active-label ownership with no safe live/progress owner; `ghost-lane diagnose` applies to missing-issue local runtime state. Stop instead of mutating when the report names live process state, run leases/shared claims, needs-attention labels, non-runtime worktree changes, unmerged commits, unavailable default-branch proof, private progress evidence, review-policy checkpoints, PR/review lineage, mixed private evidence, or unreadable worktrees.

Validate recovery by rerunning the diagnosis and `decodex status`. For review repair states, targeted `decodex run <ISSUE> --dry-run` should plan the retained repair path; if it reports no eligible issue while status still shows review repair, treat that as a runtime dispatch problem rather than adding queue labels or reusing the retained worktree.

Future agents changing recovery must preserve explicit evidence requirements and add tests under the matching `apps/decodex/src/orchestrator/tests/recovery_*` or `apps/decodex/src/recovery/tests/` area.

## Commit and landing

Manual Git lifecycle helpers are in `apps/decodex/src/cli/manual_commands.rs`:

```sh
decodex commit "summary" --authority XY-123
decodex commit "summary" --manual-authority
decodex land "summary" --authority XY-123
decodex land "summary" --manual-authority --pr <URL>
```

They produce or use `decodex/commit/2` records. The commit schema contains only `schema`, `change`, `authority`, and `impact`; PR URLs, branches, validation receipts, landing status, and closeout state belong elsewhere (`openwiki/specs/contracts-and-data.md`).

Decodex app-server runs inherit active-run commit context in their environment. That lets the owning active lane use `decodex commit "<summary>" --authority <ISSUE>` from its worktree during the handoff phase after validation, while the same command remains blocked for unrelated manual processes when the lane still has a live runtime claim.
The active-run bypass is issue-scoped: the requested commit authority must match the lane issue identifier, so `--manual-authority` and mismatched issue authorities remain blocked inside claimed lane worktrees.

Use `decodex land` rather than raw `gh pr merge` for Decodex-owned landing.

## MCP gateway

Commands (`apps/decodex/src/cli/control_commands/mcp.rs`):

```sh
decodex mcp serve --transport stdio
decodex mcp serve --transport streamable-http --listen-address 127.0.0.1:8193
decodex mcp serve --transport streamable-http --allow-origin <ORIGIN> --bearer-token-env <ENV_VAR>
decodex mcp serve --capability-profile observe|plan|operate|admin
```

Defaults:

- Stdio defaults to `admin` for local clients.
- Streamable HTTP defaults to `observe` and requires origin/bearer boundaries for non-loopback or elevated profiles.
- CORS is not auth; `Mcp-Session-Id` is protocol session state, not authorization (`apps/decodex/src/mcp.rs`).

Use MCP for typed resources, prompts, planning, lane control, and project control. Do not use it to bypass acceptance, lane-control run/turn preconditions, project enablement, or tracker/writeback policies. For remote-control watched claims, reverse checks, and stop conditions, see [Drift audits](../evidence/drift-audits.md).

## Frozen v0.2 account pool

The commands and paths in this section are frozen v0.2 provenance only. They are
excluded from the active workspace and are not bundled, installed, or invoked by the
current App. Current account and Reset Card operations use the API-only vNext CLI
described in [Commands and validation](../operations/commands-and-validation.md).

Frozen commands (`apps/decodex/src/cli/account_commands.rs`):

```sh
decodex account list --json
decodex account select <EMAIL_OR_ID_OR_FINGERPRINT>
decodex account clear
decodex account login
decodex account import-auth <AUTH_JSON>
decodex account use <SELECTOR>
decodex account logout <SELECTOR>
```

The pool is stored in `~/.codex/decodex/accounts.jsonl`; global selection/display offsets are in `~/.codex/decodex/config.toml`; `account use` overwrites Codex `auth.json` for the selected account (`apps/decodex-app/README.md`). Do not print token material.

The current native App does not read this pool or write Codex `auth.json`.

## Validation status publishing

Fast landing uses `[github].landing_mode = "fast"` plus `landing_actors` in the
registered project config. Standard landing is the default and waits for GitHub's
full status rollup. The fixed fast status context is `decodex/local-full-check`.

`decodex verify publish-status` publishes a GitHub commit status for a PR head (`apps/decodex/src/cli/verify_commands.rs`):

```sh
decodex verify publish-status \
  --config /path/to/project.toml \
  --pr https://github.com/OWNER/REPO/pull/NUMBER \
  --context decodex/local-full-check \
  --state success \
  --expected-head "$HEAD_SHA" \
  --expected-base-ref main \
  --expected-base-oid "$BASE_SHA" \
  --description "cargo make check passed"
```

Success requires current PR head, base ref, and base oid evidence, so a stale local validation run cannot publish green after the PR or target branch moved (`openwiki/operations/commands-and-validation.md`).

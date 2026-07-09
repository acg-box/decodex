# Runtime Operator Workflows

This page is the operator-task map for Decodex. It describes what commands do, where they enter source, and what future agents should verify when changing them.

## Project setup and registry

Project configs live outside target checkouts under `~/.codex/decodex/projects/<service-id>/`. Each project directory contains:

- `project.toml` for service id, tracker/GitHub credential env-var names, optional Codex/autonomy/privacy settings, and repo/worktree paths.
- `WORKFLOW.md` for execution policy.

Use the redacted example as the setup shape (`decodex.example.toml`). Do not document or read live token values.

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

`openwiki/workflows/runtime-operator-workflows.md` records current cadences: snapshots every 15 seconds; Linear-backed scans at most every 5 minutes per project unless explicitly requested. When the Decodex App starts a bundled server, it uses the same control plane and default listener (`apps/decodex-app/README.md`). Avoid running two owners on `127.0.0.1:8192`.

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
- Lane-control results and audit records are local runtime evidence; they do not replace tracker lifecycle signals.

MCP `decodex_lane_control` mirrors these preconditions as an inspect-first facade, not a shortcut.

## Program Intake

Program Intake turns accepted planning into executable issue-backed runtime state (`apps/decodex/src/cli/research_intake_commands/intake.rs`, `openwiki/specs/contracts-and-data.md`):

```sh
decodex intake goal --project <SERVICE_ID> <CONTRACT_ID> --dry-run
decodex intake goal --project <SERVICE_ID> <CONTRACT_ID> --apply
decodex intake issues --project <SERVICE_ID> <ISSUE>... --dry-run
decodex intake issues --project <SERVICE_ID> <ISSUE>... --apply
```

Rules:

- Goal intake requires an accepted Decision Contract. Draft, rejected, or needs-human-decision contracts are not executable.
- Dry-run reads and renders a deterministic report without mutating Linear or runtime Program Intake rows.
- Apply may create/update generated normal Linear issue briefs for goal intake and persist local Program Intake/Execution Program rows.
- Issue-batch apply persists local Program Intake/Execution Program state for existing issues.
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

Use `decodex land` rather than raw `gh pr merge` for Decodex-owned landing. The installable plugin hook blocks raw `git commit` and `gh pr merge` in Decodex-owned scope and tells the operator to use the Decodex commands (`plugins/decodex/scripts/decodex_lifecycle_hook`).

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

Use MCP for typed resources, prompts, planning, lane control, and project control. Do not use it to bypass acceptance, lane-control run/turn preconditions, project enablement, or tracker/writeback policies.

## Account pool and native App

Account commands (`apps/decodex/src/cli/account_commands.rs`):

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

The native App is a UI over this Rust-owned state. It is not a separate scheduler or project registry owner.

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

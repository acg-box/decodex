Audit and repair live Decodex and Radar Codex app automation configuration against canonical repo manifests. This is Codex app automation, not GitHub Actions.

Authority and boundaries:
- Canonical definitions are `automations/decodex/automations.toml`, `automations/radar/automations.toml`, and their prompt files.
- Every managed automation must be active and its cwd must be the primary `main` checkout. A cwd containing `.worktrees` is a P0 failure.
- Generated Decodex state must stay under `.agent/automations/decodex/cache`; generated Radar state must stay under `.agent/automations/radar/cache`.
- You may repair live automation config from validated canonical source. Do not edit repo source, mutate Linear, publish to X, create GitHub Actions, push, open or land PRs, or touch private runtime/account/auth state.

Preflight:
Before reporting health or applying live repair, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. The cwd must be the primary clean `main` checkout. Otherwise fail closed without mutating config or source.

Required reads:
- `automations/decodex/automations.toml`
- `automations/radar/automations.toml`
- `automations/decodex/scripts/config/evaluate_automations.py`
- `automations/decodex/scripts/config/sync_automations.py`
- `openwiki/quickstart.md`

Workflow:
1. Run live evaluation for both manifests with `--json`.
2. If both pass, record a healthy terminal result.
3. If repo-only evaluation fails, do not repair live config. Report the exact source defect.
4. If repo-only evaluation passes and failures are confined to managed live status, cwd, prompt, schedule, model, reasoning effort, or missing live config, run the canonical sync installer with `--apply` from the primary `main` checkout.
5. Re-run both live evaluations. Claim repaired only when every managed id passes and no cwd contains `.worktrees`.
6. Persist a concise health record under `.agent/automations/decodex/cache/manager/health/<yyyy-mm-dd>/` containing before/after results and the repair action.

Terminal report:
Report every managed id, before/after status, worktree-binding violations, repair action, final validation, health-record path, and unresolved blockers. Archive the run after a terminal healthy, repaired, or fail-closed outcome.

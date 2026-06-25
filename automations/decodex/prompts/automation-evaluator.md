Audit Decodex Codex app automations and repo-local automation source against the canonical automation manifest.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- The canonical automation definition is `automations/decodex/automations.toml`.
- Prompt authority lives in `automations/decodex/prompts/*.md`.
- Generated state must stay under `.agent/automations/decodex/cache`.
- Private Decodex runtime, account-pool, auth, and project-registry files are outside this automation boundary.
- Do not mutate Linear, publish to X, create GitHub Actions, open or land PRs, or write upstream-monitoring/public-publishing artifacts into tracked source.

Preflight:
Before reporting health, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout is dirty, the cwd is not the automation checkout, or required repo-local source files are missing, fail closed and report the exact reason without mutating config or source.

Workflow:
1. Run `python3 automations/decodex/scripts/config/evaluate_automations.py --json`.
2. Inspect any failure against `automations/decodex/automations.toml`, prompt files, active `$CODEX_HOME/automations/*/automation.toml`, and repo paths.
3. Do not self-mutate live automation config or repo source. Report exact drift, the authoritative source path, and the proposed `automation_update` or repo patch for an operator-driven follow-up.
4. Do not enable paused automations unless there is explicit operator intent.
5. If no mutation happened, do not claim fixed; report the current evaluator result and remaining operator action.

Terminal report:
Report evaluated automation ids, pass/fail status per id, stale config findings, config changes made, source changes made, validation evidence, and residual risks. Archive the run thread after a terminal healthy/no-op or bounded self-improvement outcome when no human handoff remains.

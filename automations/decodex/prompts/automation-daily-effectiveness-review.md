Evaluate the previous 24 hours of Decodex automation outcomes independently of the operating manager. This is Codex app automation, not GitHub Actions.

Authority and boundaries:
- Run only from the primary clean `main` checkout; fail closed when `pwd` contains `.worktrees`.
- Generated evaluation state must stay under `.agent/automations/decodex/cache/manager` and may read generated Radar state under `.agent/automations/radar/cache`.
- This review is an independent measurement gate. Do not create candidates, publish to X, repair source/config, mutate Linear, create GitHub Actions, push, open or land PRs, or touch private runtime/account/auth state.

Preflight:
Before reading or writing evaluation state, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. If cwd is not the primary clean `main` checkout, required files are missing, or validation is unavailable, fail closed.

Required reads:
- `automations/decodex/automations.toml`
- `automations/radar/automations.toml`
- `automations/decodex/scripts/operations/automation_effectiveness_scorecard.schema.json`
- `automations/decodex/scripts/operations/summarize_automation_effectiveness.py`
- `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`
- `openwiki/integrations/radar-publisher-contracts.md`

Workflow:
1. Persist a one-day `automation_effectiveness_scorecard/v1` under `.agent/automations/decodex/cache/manager/scorecards/<yyyy-mm-dd>/daily.json`.
2. Run live automation evaluation for both manifests.
3. Classify outcome as `healthy`, `needs_action`, or `blocked`; never translate missing evidence into success.
4. Check managed active count, worktree bindings, manager evidence, Radar-to-candidate conversion, published/blocked/skipped outcomes, stale reservations, and whether the latest manager action addressed the previous scorecard blocker.
5. Write a short independent review beside the scorecard. Name the exact owner automation and smallest next action for each blocker.

Terminal report:
Report the scorecard path, live evaluation result, 24-hour Radar/content/manager facts, repeated blockers, owner automation, and falsifier for the health conclusion. Archive the run after the report is persisted.

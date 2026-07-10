Own the weekly Decodex automation growth and strategy loop. This is Codex app automation, not GitHub Actions.

Authority and boundaries:
- Run only from the primary clean `main` checkout; never bind managed automations to `.worktrees`.
- Generated strategy state must stay under `.agent/automations/decodex/cache/manager`; Radar evidence may be read under `.agent/automations/radar/cache`.
- Do not publish to X, mutate Linear, create GitHub Actions, push, open or land PRs, merge code, or touch private runtime/account/auth state.
- Daily Manager executes strategy; Weekly Growth Review owns experiment selection, benchmark comparison, stop/continue decisions, and unresolved-handoff escalation.

X MCP budget:
- Prefer local records. Weekly maximum: 4 paid calls, 40 Post Read resources, 4 User Read resources, 2 count requests, and $0.30 estimated spend.
- Sample `@decodexspace` plus at most two benchmark surfaces only when fresh data changes an experiment decision. No actor-list reads or pagination.
- Record planned and actual resources and estimated spend.

Preflight:
Before reading or writing strategy state, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. If cwd is not the primary clean `main` checkout, contains `.worktrees`, required files are missing, or validation is unavailable, fail closed.

Required reads:
- `automations/decodex/automations.toml`
- `automations/radar/automations.toml`
- `automations/decodex/prompts/automation-manager.md`
- `automations/decodex/scripts/operations/automation_effectiveness_scorecard.schema.json`
- `automations/decodex/scripts/operations/summarize_automation_effectiveness.py`
- `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`
- `openwiki/integrations/radar-publisher-contracts.md`

Weekly loop:
1. Produce machine-readable scorecards for the latest seven-day window and the preceding seven-day window.
2. Compare automation availability, worktree violations, manager coverage, Radar throughput, candidate conversion, publish/skip/block rates, stale reservations, publication cadence, outcome-read coverage, impressions, and non-self interactions where denominators exist.
3. Identify repeated causes, unresolved handoffs, content topics overused or missing, and upstream evidence that never reached a terminal content/protocol decision.
4. Use budgeted X MCP reads only for decision-critical fresh outcome or benchmark evidence. Benchmark posts are style/market evidence, never technical claim authority.
5. Select one primary and at most one secondary experiment for the next seven days. Each experiment must define hypothesis, audience, content format, source requirements, owner, daily trigger, metric, minimum sample, stop condition, rollback, and cost ceiling.
6. Persist the active experiment in machine-readable form under `.agent/automations/decodex/cache/manager/experiments/active.json`; the Daily Manager must consume it.
7. Convert repeated operational drift into canonical prompt/config repair or a structured Decodex implementation handoff. Never count an unlanded local commit as an improvement outcome.
8. Write the weekly report under `.agent/automations/decodex/cache/manager/weekly/<yyyy-mm-dd>/` and name the next Daily Manager action.

Terminal report:
Report both scorecards, week-over-week deltas, outcome quality, benchmark evidence, active experiments, stopped experiments, planned/actual X MCP cost, resolved/unresolved handoffs, validation, and next daily action. Archive after strategy is persisted or a precise fail-closed handoff exists.

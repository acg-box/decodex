# Decodex Publisher Automation Operations

This directory owns repo-local source for Decodex Publisher and automation-operations jobs.

- `automations.toml`: checked-in recurring automation source for public publishing,
  health repair, daily effectiveness, daily management, and weekly growth review.
- `prompts/`: Codex app automation prompts for Publisher-owned jobs.
- `scripts/social/`: social candidate, reservation, and post schemas.
- `skills/`: Publisher skills and shared publishing gates.
- `scripts/config/`: shared automation config evaluation and live-install utilities.
- `scripts/operations/`: deterministic effectiveness scorecards consumed by daily and
  weekly management loops.
- `research/`: retained automation research data that is not part of OpenWiki.

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`.

Publisher owns `social_candidate/v1`, `social_publish_reservation/v1`, and
`social_post/v1`. It consumes Radar handoff evidence from
`.agent/automations/radar/cache`, but it must not refresh upstream state or perform
fresh upstream source analysis.

Radar automation source lives under `automations/radar/`.

## Portable Codex App Install

The repo intentionally keeps the portable automation source in `automations.toml` plus
prompt files instead of checking in live `$CODEX_HOME/automations/*/automation.toml`
files. Live Codex app configs contain machine-local fields such as absolute checkout
paths and timestamps.

Install or refresh the live Codex app automation configs from a clone with:

```sh
python3 automations/decodex/scripts/config/sync_automations.py --apply
```

Dry-run without writing:

```sh
python3 automations/decodex/scripts/config/sync_automations.py
```

Validate the installed live configs against repo authority:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/radar/automations.toml
```

The installer resolves `cwd = "{repo_root}"` to the primary checkout owning `main`,
even when the command is invoked from a development worktree. It rejects explicit
linked-worktree runtime roots. The evaluator also fails any managed live config whose
cwd contains `.worktrees`. Prompts containing configured private fragments such as
absolute user-home paths, auth files, account files, or runtime databases are refused.

Build a deterministic seven-day operating scorecard with:

```sh
python3 automations/decodex/scripts/operations/summarize_automation_effectiveness.py
```

The scorecard reports live availability and cwd violations, social candidate and
terminal outcomes, stale reservations, Radar throughput, manager evidence, and
machine-actionable blockers. It does not substitute generated evidence for X outcome
readback or accepted Decodex execution authority.

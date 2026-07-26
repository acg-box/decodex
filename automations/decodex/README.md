# Decodex Automation Support

This directory owns the minimal Decodex content loop, Publisher contracts, skills,
and shared Codex App automation configuration tools.

- `automations.toml`: current Content Manager and browser Publisher tasks.
- `prompts/`: current task contracts.
- `scripts/social/`: social candidate, reservation, post, outcome, and strategy
  schemas.
- `skills/`: Publisher skills and shared publishing gates.
- `scripts/config/`: shared automation config evaluation and live-install utilities.
- `research/`: retained automation research data.

The obsolete v0.2 task graph, prompts, and effectiveness scorecard were removed. The
current two-task manifest is a new contract and does not consume legacy task state.

Generated content state belongs under `.agent/automations/decodex/cache`. Social,
strategy, browser-session, and browser-lease records are local-only. Never commit,
upload, or archive them to Git.

Publisher owns `social_candidate/v1`, `social_publish_reservation/v1`,
`social_post/v1`, `social_outcome/v1`, browser lease serialization, and publication
validation. Content Manager owns `social_strategy/v1`. Publisher consumes Radar
handoff evidence from `.agent/automations/radar/cache`, but it must not refresh
upstream state or perform fresh upstream source analysis.

The standalone upstream adaptation loop lives under `automations/upstream/`. Radar
remains an auxiliary evidence tool and is not a separate scheduled task. Content
Manager invokes Radar when it needs source-backed evidence.

## Portable Codex App Install

The repo keeps portable sources in `automations/upstream/automations.toml`,
`automations/decodex/automations.toml`, and their prompt files instead of checking in
live `$CODEX_HOME/automations/*/automation.toml` files. Live Codex App configs contain
machine-local fields such as absolute checkout paths and timestamps.
The live evaluator requires positive `created_at` and `updated_at` values and rejects
an update timestamp earlier than creation. This prevents managed tasks from disappearing
from the Codex App list while remaining addressable by ID.

Render the current default automation configs from a clone with:

```sh
python3 automations/decodex/scripts/config/sync_automations.py --apply
```

Dry-run without writing:

```sh
python3 automations/decodex/scripts/config/sync_automations.py
```

Validate the installed live configs against current repo authority:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/upstream/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
```

The installer resolves `cwd = "{repo_root}"` to the primary checkout owning `main`,
even when the command is invoked from a development worktree. It rejects explicit
linked-worktree runtime roots. The evaluator also fails any managed live config whose
cwd contains `.worktrees`. Prompts containing configured private fragments such as
absolute user-home paths, auth files, account files, or runtime databases are refused.

Read the bounded upstream state and health result with:

```sh
python3 automations/upstream/scripts/upstream_autopilot.py snapshot --json
python3 automations/upstream/scripts/upstream_autopilot.py health --json
```

The native Codex Desktop automation lifecycle tool is the normal live mutation path.
The renderer remains a portable recovery path and preserves Codex App timestamps.

## Scheduled Run Threads

Each scheduled execution creates a Codex thread. All five managed tasks apply
`skills/references/scheduled-run-thread-retention.md` after their final durable
readback. A terminal run with no remaining lease, handoff, ambiguous external effect,
or human decision calls native `set_thread_archived` for its current thread. A run
that needs attention stays visible.

This action does not pause or delete the recurring task, and it does not remove local
evidence. Never use run-thread archiving to hide a failed or incomplete operation.

# Decodex Automation Support

This directory retains Publisher schemas, skills, and shared Codex App automation
configuration tools.

- `scripts/social/`: social candidate, reservation, and post schemas.
- `skills/`: Publisher skills and shared publishing gates.
- `scripts/config/`: shared automation config evaluation and live-install utilities.
- `research/`: retained automation research data.

The obsolete v0.2 manifest, prompts, and effectiveness scorecard were removed. They
are not compatibility inputs for the current automation system.

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`.

Publisher owns `social_candidate/v1`, `social_publish_reservation/v1`, and
`social_post/v1`. It consumes Radar handoff evidence from
`.agent/automations/radar/cache`, but it must not refresh upstream state or perform
fresh upstream source analysis.

The current standalone upstream adaptation loop lives under `automations/upstream/`.
Radar remains an auxiliary evidence tool and is not a default scheduled task.

## Portable Codex App Install

The repo keeps the current portable source in
`automations/upstream/automations.toml` and its prompt files instead of checking in
live `$CODEX_HOME/automations/*/automation.toml` files. Live Codex App configs contain
machine-local fields such as absolute checkout paths and timestamps.

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

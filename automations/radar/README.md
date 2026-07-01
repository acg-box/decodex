# Radar Automation Operations

This directory owns repo-local source for the Radar auxiliary tool and recurring
upstream evidence automation.

- `automations.toml`: checked-in recurring automation source for upstream review,
  release checkpoint curation, and Radar artifact retention.
- `radar.toml`: canonical Radar cache and handoff path contract.
- `prompts/`: Codex app automation prompts for Radar-owned jobs.
- `scripts/github/`: bounded GitHub and Codex analysis helper contracts.
- `skills/`: repo-local Radar skills for upstream triage, code analysis, release
  analysis, and signal drafting.

Generated Radar state belongs under `.agent/automations/radar/cache`.

Radar owns upstream evidence, `upstream_review/v1`, `upstream_impact/v1`,
`analysis_draft`, `signal_entry/v1`, `release_delta/v1`, and
`control_plane_upgrade_candidate/v1` artifacts. It does not own Decodex runtime
commands or Decodex social publication artifacts.

Decodex Publisher consumes Radar handoff evidence and owns
`social_candidate/v1`, `social_publish_reservation/v1`, and `social_post/v1` under
`.agent/automations/decodex/cache/social`.

Live Codex app automation configs for these Radar jobs are generated from this
directory's `automations.toml` by
`automations/decodex/scripts/config/sync_automations.py`. Keep repo source portable:
use relative paths and `{repo_root}` placeholders here, and let the installer resolve
machine-local checkout paths under `$CODEX_HOME/automations` on each host.

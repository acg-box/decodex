# Radar Automation Operations

This directory owns reusable repo-local source for the Radar auxiliary evidence tool.

- `radar.toml`: canonical Radar cache and handoff path contract.
- `scripts/github/`: bounded GitHub and Codex analysis helper contracts.
- `skills/`: repo-local Radar skills for upstream triage, code analysis, release
  analysis, and signal drafting.

The obsolete Radar schedule and prompts were removed. The current upstream adaptation
loop does not depend on them.

Generated Radar state belongs under `.agent/automations/radar/cache`.

Radar owns upstream evidence, `upstream_review/v1`, `upstream_impact/v1`,
`analysis_draft`, `signal_entry/v1`, `release_delta/v1`, and
`control_plane_upgrade_candidate/v1` artifacts. It does not own Decodex runtime
commands or Decodex social publication artifacts.

Decodex Publisher consumes Radar handoff evidence and owns
`social_candidate/v1`, `social_publish_reservation/v1`, and `social_post/v1` under
`.agent/automations/decodex/cache/social`.

The current upstream adaptation tasks live under `automations/upstream/`. The default
sync path does not generate live Radar jobs from this directory.

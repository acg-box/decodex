# Decodex Publisher Automation Operations

This directory owns repo-local source for Decodex Publisher automation.

- `automations.toml`: checked-in recurring automation source for public-publishing and
  automation health audit jobs.
- `prompts/`: Codex app automation prompts for Publisher-owned jobs.
- `scripts/social/`: social candidate, reservation, and post schemas.
- `skills/`: Publisher skills and shared publishing gates.
- `scripts/config/`: shared automation config evaluation utilities.
- `research/`: retained automation research data that is not part of the Markdown docs
  bundle.

Generated Publisher state belongs under `.agent/automations/decodex/cache/social`.

Publisher owns `social_candidate/v1`, `social_publish_reservation/v1`, and
`social_post/v1`. It consumes Radar handoff evidence from
`.agent/automations/radar/cache`, but it must not refresh upstream state or perform
fresh upstream source analysis.

Radar automation source lives under `automations/radar/`.

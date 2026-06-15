---
name: labels
description: Use when Decodex labels affect intake.
---

# Labels

Handle Decodex Linear labels without changing runtime ownership by accident. Read
`../../references/routing.md` for service-id and recovery details.

- `decodex:queued:<service-id>`: ordinary issue intake candidate.
- `decodex:active:<service-id>`: runtime-owned active lane marker.
- `decodex:manual-only`: human opt-out.
- `decodex:needs-attention`: human-required stop.
- Read `<service-id>` from `project.toml`.
- Queue only issues startable under `WORKFLOW.md`.
- Require promoted research or explicit execution intent before research-derived
  intake.

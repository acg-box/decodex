---
type: "Runbook"
title: "Review Config Migration"
description: "One-time manual migration from the historical Decodex review config keys to the v0.2.0 review-level model. Use this when: An agent or operator is updating a registered Decodex project `project.toml` for the Loop Engineering release. Do not use this for: Changing review behavior in code, adding compatibility parsing, or mutating operator-local dogfood config before the release flip issue says to do so. Governing specs: [`../spec/review-orchestration.md`](../spec/review-orchestration.md) and [`../spec/runtime.md`](../spec/runtime.md)."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-07-07
---
# Review Config Migration

Purpose: One-time manual migration from the historical Decodex review config keys to
the v0.2.0 review-level model.
Use this when: An agent or operator is updating a registered Decodex project
`project.toml` for the Loop Engineering release.
Do not use this for: Changing review behavior in code, adding compatibility parsing,
or mutating operator-local dogfood config before the release flip issue says to do so.
Governing specs: [`../spec/review-orchestration.md`](../spec/review-orchestration.md)
and [`../spec/runtime.md`](../spec/runtime.md).

## Target Shape

Every migrated project config uses only this review key:

```toml
[codex]
review = "off" # or "standard", "strict"
```

The levels mean:

- `off`: no review gate.
- `standard`: Decodex Review through `issue_review_checkpoint`.
- `strict`: Standard plus GitHub Review through the existing `@codex review` path.

## Historical Mapping

Use the old fields only as migration input:

| Old fields | New level |
| --- | --- |
| `internal_review_mode = "off"` and `external_review_enabled = false` | `review = "off"` |
| `internal_review_mode = "prompt"` and `external_review_enabled = false` | `review = "off"` if no independent gate is required, otherwise `review = "standard"` |
| `internal_review_mode = "loop"` and `external_review_enabled = false` | `review = "standard"` |
| `internal_review_mode = "loop"` and `external_review_enabled = true` | `review = "strict"` |

If an old config combined prompt-only local review with GitHub Review, choose the new
level by intent: use `standard` to keep an independent Decodex Review gate or
`strict` to keep the GitHub Review path. The new model intentionally does not
preserve prompt-only review as a supported cross-product.

## Files To Inspect

- Registered project config: `~/.codex/decodex/projects/<service-id>/project.toml`
- Registered project workflow: `~/.codex/decodex/projects/<service-id>/WORKFLOW.md`
- Checked-in example: `decodex.example.toml`
- Review specs: `docs/spec/review-orchestration.md`,
  `docs/spec/runtime.md`, and `docs/spec/tracker-tools.md`
- Prompt and tracker behavior if changing code:
  `apps/decodex/src/orchestrator/prompting.rs` and
  `apps/decodex/src/agent/tracker_tool_bridge/`

## No-Compat Rule

Do not add or rely on fallback parsing for the old keys. After this migration, the
project config parser should reject unknown historical review fields through the
normal strict TOML schema.

Do not mutate the operator-local Decodex dogfood config until the new code is merged
and the final dogfood or release issue is ready to flip it. That config currently
belongs to the operator environment, not this source-lane migration.

## Validation

After code or checked-in docs change, run:

```sh
cargo test -p decodex review --all-features -- --test-threads=1
cargo test -p decodex config --all-features -- --test-threads=1
cargo test -p decodex normal_prompts --all-features -- --test-threads=1
cargo make fmt
cargo make lint-fix
cargo make test
```

Before pushing a PR head, the registered project gate is the canonicalize commands
followed by the verify commands from `WORKFLOW.md`.

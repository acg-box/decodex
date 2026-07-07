# Runbook Index

Purpose: Route agents to procedural documents that tell them which sequence to execute.

Question this index answers: "which sequence should I execute?"

## Use this index when

- You need a runbook, how-to, migration sequence, validation flow, troubleshooting path,
  or maintenance procedure.
- You already know the relevant spec and need the operational steps.
- You need explicit prerequisites, commands, checkpoints, or verification.

## Do not use this index when

- You need the authoritative contract, schema, or invariant.
- You need current repository layout or implementation boundaries.
- You need durable design rationale rather than operator steps.

## What belongs in `docs/runbook/`

- Task-oriented operator procedures.
- Validation and inspection sequences.
- Rollout, rollback, and recovery flows.
- Bounded recipes that depend on a governing spec.

## Current runbooks

- [`autonomy-implementation-roadmap.md`](./autonomy-implementation-roadmap.md) for
  implementing objective-driven Decodex autonomy from Objective Contracts through
  signals, proposal dry-runs, Decision Contract promotion, Program Intake, operator
  readback, MCP exposure, and self-dogfood.
- [`control-plane-upgrade-workflow.md`](./control-plane-upgrade-workflow.md) for
  operating the bridge from upstream Codex Radar evidence to Decodex Control Plane
  upgrade candidates, Decision Contract promotion, and Program Intake.
- [`github-pages-deploy.md`](./github-pages-deploy.md) for GitHub Pages deployment and
  `decodex.space` custom-domain setup for the static public site.
- [`linear-archive-hygiene.md`](./linear-archive-hygiene.md) for dry-run-first
  archive hygiene of old terminal Linear issues by repo label.
- [`lane-control-recovery.md`](./lane-control-recovery.md) for deciding whether to
  inspect, resume, scan, keep or remove queue labels, or route manual attention after
  interrupt, hard fallback, broad steer, task replacement, or ambiguous recovery
  evidence.
- [`mcp-remote-control.md`](./mcp-remote-control.md) for running Streamable HTTP MCP
  with loopback defaults, CORS trust, capability profiles, public-safe observation,
  canonical refusal paths, and the current auth/process-smoke gaps.
- [`orchestration-kernel-cutover.md`](./orchestration-kernel-cutover.md) for the
  direct runtime cutover from scattered lane lifecycle decisions to a single typed
  orchestration kernel with checklist, subagent review, and validation gates.
- [`recover-review-handoff.md`](./recover-review-handoff.md) for diagnosing retained
  review lanes, explicitly rebinding missing or stale runtime DB lifecycle records, and
  adopting verified human-owned PRs into the normal Decodex landing lifecycle.
- [`review-config-migration.md`](./review-config-migration.md) for one-time migration
  from historical review config keys to `[codex].review` levels.
- [`release-readiness.md`](./release-readiness.md) for the v0.2.0 Loop Engineering
  release-candidate gate, dogfood evidence checklist, tag contract, and release note.
- [`self-dogfood-pilot.md`](./self-dogfood-pilot.md) for the retained-lane pilot run
  against `decodex` itself and the bounded live-operation sequence.

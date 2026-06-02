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

- [`github-pages-deploy.md`](./github-pages-deploy.md) for GitHub Pages deployment and
  `decodex.space` custom-domain setup for the static public site.
- [`linear-archive-hygiene.md`](./linear-archive-hygiene.md) for dry-run-first
  archive hygiene of old terminal Linear issues by repo label.
- [`lane-control-recovery.md`](./lane-control-recovery.md) for deciding whether to
  inspect, resume, scan, keep or remove queue labels, or route manual attention after
  interrupt, hard fallback, broad steer, task replacement, or ambiguous recovery
  evidence.
- [`local-github-signal-workflow.md`](./local-github-signal-workflow.md) for collecting
  GitHub change bundles, running Codex editorial analysis, validating signal entries,
  and publishing static site content.
- [`radar-artifact-archive.md`](./radar-artifact-archive.md) for moving raw Radar
  bundles and analysis drafts out of Git after the 21-day hot window while keeping
  release-asset recovery manifests checked in.
- [`social-publishing-workflow.md`](./social-publishing-workflow.md) for turning Radar
  evidence into low-frequency `@decodexspace` X posts or blocked publication records.
- [`recover-review-handoff.md`](./recover-review-handoff.md) for diagnosing and
  explicitly rebinding retained review lanes blocked by a missing or stale runtime DB
  handoff
  marker.
- [`self-dogfood-pilot.md`](./self-dogfood-pilot.md) for the retained-lane pilot run
  against `decodex` itself and the bounded live-operation sequence.

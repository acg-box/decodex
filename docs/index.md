# Documentation Index

Purpose: Route agents to the smallest Decodex-owned documentation surface.

Audience: Agents and maintainers working on the Decodex runtime, native app, static
site, operator tooling, and repository support code.

## Read Order

- Read `README.md` for repository scope and top-level ownership.
- Read `docs/reference/build-test-run.md` for setup, build, test, run, and validation
  entrypoints.
- Read `docs/policy.md` for documentation ownership and writing rules.
- Read `plugins/decodex/skills/decodex/SKILL.md` for Decodex runtime/operator plugin
  routing.
- Then choose the narrowest lane:
  - `docs/spec/index.md` for required Decodex behavior.
  - `docs/runbook/index.md` for operator procedures.
  - `docs/reference/index.md` for current implementation maps.
  - `docs/decisions/index.md` for Decodex-specific rationale.
  - `docs/evidence/index.md` for reusable public-safe proof.

## Routing Matrix

- Runtime contracts, invariants, schemas, state machines, or required behavior:
  `docs/spec/`
- Objective-driven project autonomy and proposal boundaries:
  `docs/spec/autonomy-control-plane.md`
- Operator lane control, pause/resume, scan, interrupt, steer, retained retry/resume,
  or manual attention: `docs/spec/lane-control.md`
- Post-control recovery after interrupt, fallback, broad steer, task replacement, or
  ambiguous retained evidence: `docs/runbook/lane-control-recovery.md`
- Static public site contracts: `docs/spec/site-contract.md`
- Runbooks, migrations, validation steps, troubleshooting, or operational sequences:
  `docs/runbook/`
- Current repository layout, ownership boundaries, static-site/runtime split, or
  implementation surface maps: `docs/reference/`
- Setup, build, test, run, validation, task names, automation entrypoints, or local
  source commands: `docs/reference/build-test-run.md` and `Makefile.toml`
- Durable Decodex design rationale, packaging choices, MCP integration boundaries, or
  static-site tradeoffs: `docs/decisions/`
- Reusable public-safe evidence: `docs/evidence/index.md`
- Reusable Decodex agent plugin instructions: `plugins/decodex/`
- Generic repository execution, knowledge maintenance, or external research methods:
  external installed team plugins, not Decodex-owned docs.

## Retrieval Rules

- Optimize for direct task execution.
- Keep runtime authority explicit: `apps/decodex/src/`, registered project contracts
  under `~/.codex/decodex/projects/<service-id>/`, and `docs/spec/` outrank runbooks,
  reference material, and decisions.
- Keep the public site static by default. `site/` must not depend on a live Decodex
  daemon unless a later Decodex decision changes that boundary.
- Keep links explicit and stable.
- Keep generic knowledge/research workflow instructions out of this repository unless
  they are Decodex-specific product/runtime requirements.

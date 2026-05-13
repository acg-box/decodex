# Documentation Index

Purpose: Route agents to the smallest correct repository surface for the current task.

Audience: All documentation in this repository is written for AI agents and LLM workflows.
The split below is by question type, not by human-versus-agent audience.

## Read order

- Read `README.md` first when you need the repository scope, top-level layout, or
  current source-of-truth boundaries.
- Use `cargo make` whenever an equivalent repo task exists. When task details matter,
  inspect `Makefile.toml` directly or run `cargo make --list-all-steps`.
- Read `docs/policy.md` for document contracts, placement rules, and naming rules.
- Read the registered project `WORKFLOW.md` under
  `~/.codex/decodex/projects/<service-id>/` when the question is about validation,
  tracker routing, or execution policy.
- Read `plugins/decodex/skills/decodex/SKILL.md` when the question is how an agent
  should use Decodex in manual CLI mode or runtime-owned automation mode.
- Then choose one primary lane:
  - `docs/spec/index.md` when the question is "what must be true?"
  - `docs/runbook/index.md` when the question is "which sequence should I execute?"
  - `docs/reference/index.md` when the question is "how is it currently organized or
    implemented?"
  - `docs/decisions/index.md` when the question is "why was it designed this way?"
- Use `docs/plans/` only when a planning tool or historical execution workflow
  explicitly points to a saved plan artifact there.

## Routing matrix

- Need runtime contracts, invariants, schemas, enums, state machines, or required
  behavior -> `docs/spec/`
- Need public static-site contracts, GitHub bundle schemas, signal-entry schemas, or
  release-delta schemas -> `docs/spec/`
- Need runbooks, migrations, validation steps, troubleshooting, or operational
  sequences -> `docs/runbook/`
- Need current repository layout, ownership boundaries, static-site/runtime split, or
  implementation surface maps -> `docs/reference/`
- Need durable design rationale, packaging choices, or static-site tradeoffs ->
  `docs/decisions/`
- Need the current Radar, Control Plane, and Publisher capability boundary ->
  `docs/decisions/radar-control-plane-publisher.md`
- Need Radar raw-artifact retention, archive manifests, or GitHub Release archive
  procedure -> `docs/spec/radar-artifact-retention.md` and
  `docs/runbook/radar-artifact-archive.md`
- Need the raw machine-authored research run artifacts used by shipped research tooling
  -> `docs/research/`
- Need reusable agent-facing Decodex usage instructions -> `plugins/decodex/`
- Need repo-local Radar skills for upstream Codex triage, code analysis, release
  analysis, GitHub signal drafting, or X post drafting -> `dev/skills/` plus
  `docs/runbook/local-github-signal-workflow.md`
- Need upstream Codex impact classification or social post draft contracts ->
  `docs/spec/upstream-impact.md` and `docs/spec/social-post-draft.md`
- Need the `@decodexspace` social publishing procedure ->
  `docs/runbook/social-publishing-workflow.md`
- Need repository execution defaults or tracker-state policy -> registered project
  `WORKFLOW.md`
- Need repo task names or automation entrypoints -> `Makefile.toml`
- Need historical saved execution plans from the original static-site bootstrap ->
  `docs/plans/`

## Retrieval rules

- Optimize for agent routing and execution, not narrative flow.
- Read `docs/policy.md` for lane ownership and authoring rules.
- Keep one authoritative document per topic. Link instead of copying.
- Keep runtime authority explicit: `apps/decodex/src/`, registered project contracts
  under `~/.codex/decodex/projects/<service-id>/`, and `docs/spec/` outrank runbook,
  reference, and decision material.
- Keep the public site static by default. `site/` consumes checked-in content and
  generated JSON; it must not depend on a live Decodex daemon unless a later decision
  changes that boundary.
- Keep social publishing static-first as well. Drafts must be reviewable checked-in
  artifacts before any external posting automation acts on them.
- Start each document with a short routing header that says what the document is for,
  when to read it, and what it does not cover.
- Keep links explicit and stable.
- Treat `docs/research/` and `docs/plans/` as supporting or historical evidence, not as
  primary authority lanes.

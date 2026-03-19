# Documentation Index

Purpose: Route agents to the smallest correct document set for the current task.

Audience: All documentation in this repository is written for AI agents and LLM workflows.
The split below is by question type, not by human-versus-agent audience.

## Read order

- Read `docs/governance.md` for document contracts and placement rules.
- Read `Makefile.toml` when the task depends on repo task names or execution entrypoints.
- Then choose one primary lane:
  - `docs/spec/index.md` when the question is "what must be true?"
  - `docs/guide/index.md` when the question is "what should I do?"
- Use `docs/plans/` only when a planning tool or execution workflow explicitly points to
  a saved plan artifact there.

## Routing matrix

- Need contracts, invariants, schemas, enums, state machines, or required behavior ->
  `docs/spec/`
- Need runbooks, migrations, validation steps, troubleshooting, or operational sequences ->
  `docs/guide/`
- Need repo task names or automation entrypoints -> `Makefile.toml`
- Need documentation placement or authoring rules -> `docs/governance.md`
- Need a planning-tool artifact or saved execution plan -> `docs/plans/`

## Retrieval rules

- Optimize for agent routing and execution, not narrative flow.
- Keep one authoritative document per topic. Link instead of copying.
- Start each document with a short routing header that says what the document is for,
  when to read it, and what it does not cover.
- Keep links explicit and stable.
- Let structure emerge from real topics. Do not create empty folders, empty indexes, or
  naming schemes that are stricter than the current corpus needs.

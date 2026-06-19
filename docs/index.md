# Documentation Index

Purpose: Route agents to the smallest correct repository surface for the current task.

Audience: All documentation in this repository is written for AI agents and LLM workflows.
The split below is by question type, not by human-versus-agent audience.

## Read order

- Read `README.md` first when you need the repository scope, top-level layout, or
  current source-of-truth boundaries.
- Read `docs/reference/build-test-run.md` when the question is how to set up, build,
  test, run, or validate this repository.
- Use `cargo make` whenever an equivalent repo task exists. When task details matter,
  inspect `Makefile.toml` directly or run `cargo make --list-all-steps`.
- Read `docs/policy.md` for document contracts, placement rules, and naming rules.
- Read the registered project `WORKFLOW.md` under
  `~/.codex/decodex/projects/<service-id>/` when the question is about validation,
  tracker routing, or execution policy.
- Read `plugins/decodex/skills/decodex/SKILL.md` when the question is how an agent
  should plan for Decodex, use manual CLI mode, or use runtime-owned automation mode.
- Then choose one primary lane:
  - `docs/spec/index.md` when the question is "what must be true?"
  - `docs/runbook/index.md` when the question is "which sequence should I execute?"
  - `docs/reference/index.md` when the question is "how is it currently organized or
    implemented?"
  - `docs/decisions/index.md` when the question is "why was it designed this way?"
  - `docs/evidence/index.md` when the question is "which reusable public-safe proof
    supports this claim?"
  - `docs/research/index.md` when the question is "which latent research concept or
    supporting evidence exists but is not repository authority yet?"
  - `docs/evidence/index.md` plus `plugins/knowledge/skills/docs-drift/SKILL.md` when
    the question is "which semantic-drift audit checks docs, code, commands, status,
    and examples against each other?"

## Routing matrix

- Need runtime contracts, invariants, schemas, enums, state machines, or required
  behavior -> `docs/spec/`
- Need the natural-language-first loop-runtime contract, Research/Decision stage,
  latent Loop/Decision Contract, internal Execution Program, phase-scoped goals,
  unattended execution behavior, or loop guardrails -> `docs/spec/loop-runtime.md`
- Need current Decodex/Codex app-server protocol support
  evidence -> `docs/spec/app-server.md`
- Need Decodex operator lane-control capability support, including inspect,
  pause/resume, scan, interrupt, steer, retained retry/resume, manual attention, or
  unsupported/deferred controls -> `docs/spec/lane-control.md`
- Need the post-control recovery sequence after lane interrupt, hard fallback, broad
  steer, task replacement, or ambiguous retained evidence ->
  `docs/runbook/lane-control-recovery.md`
- Need public static-site contracts -> `docs/spec/site-contract.md`
- Need runbooks, migrations, validation steps, troubleshooting, or operational
  sequences -> `docs/runbook/`
- Need current repository layout, ownership boundaries, static-site/runtime split, or
  implementation surface maps -> `docs/reference/`
- Need repo setup, build, test, run, validation, task names, automation entrypoints, or
  local source commands -> `docs/reference/build-test-run.md` and `Makefile.toml`
- Need durable design rationale, packaging choices, or static-site tradeoffs ->
  `docs/decisions/`
- Need rationale for keeping execution-graph semantics internal behind a
  natural-language user surface ->
  `docs/decisions/natural-language-loop-runtime.md`
- Need rationale for OKF research promotion, research concept disposition, or LLM Wiki
  retrieval hygiene -> `docs/decisions/okf-research-knowledge-lifecycle.md`
- Need rationale for Decodex MCP integration, MCP/skills/docs/runtime boundaries, or
  skill slimming -> `docs/decisions/mcp-capability-gateway-and-skill-slimming.md`
- Need research concepts, supporting research evidence, or the implemented/superseded
  status of candidate research targets ->
  `docs/reference/research-concepts.md`
- Need reusable public-safe evidence concepts -> `docs/evidence/index.md`
- Need new Decodex bounded research, design investigation, evidence ledger, or
  research-to-execution promotion -> `plugins/decodex/skills/research/`,
  `plugins/agent-method/skills/challenge/`,
  `plugins/decodex/skills/research-promote/`, and `docs/spec/loop-runtime.md`
- Need docs-impact classification `research_required` -> switch from the docs skill
  to `plugins/decodex/skills/research/` and
  `plugins/agent-method/skills/challenge/`;
  checked-in output under `docs/research/` stays latent and non-authoritative until
  promoted into `spec`, `runbook`, `reference`, `decisions`, or `evidence`
- Need a semantic-drift audit concept, stale-claim evidence, or docs/code alignment
  verdict -> `docs/evidence/index.md` and `plugins/knowledge/skills/docs-drift/SKILL.md`
- Need docs maintenance, OKF concepts, docs impact classification, or drift gate
  handling -> `plugins/knowledge/skills/docs/SKILL.md` and `docs/policy.md`
- Need autonomous Decodex context intake, OKF/LLM Wiki owner reads, context anchors,
  or late docs-skill recovery ->
  `plugins/decodex/references/routing.md`
- Need the current docs knowledge map, OKF/LLM Wiki value evaluation, graph
  maintenance anchors, or owner-coverage observations ->
  `docs/reference/docs-knowledge-map.md`
- Need OKF concept schema, LLM Wiki navigation, or drift audit details ->
  `plugins/knowledge/references/docs-okf.md`,
  `plugins/knowledge/references/docs-wiki.md`, and
  `plugins/knowledge/skills/docs-drift/SKILL.md`
- Need reusable agent-facing Decodex usage instructions -> `plugins/decodex/`
- Need repository execution defaults or tracker-state policy -> registered project
  `WORKFLOW.md`
- Need repo task names or automation entrypoints -> `docs/reference/build-test-run.md`
  and `Makefile.toml`

## Retrieval rules

- Optimize for agent navigation and execution, not narrative flow.
- Read `docs/policy.md` for lane ownership and authoring rules.
- Use `plugins/knowledge/skills/docs/SKILL.md` when a Decodex lane touches docs or
  changes behavior that may require docs impact classification.
- Keep one authoritative document per topic. Link instead of copying.
- Keep runtime authority explicit: `apps/decodex/src/`, registered project contracts
  under `~/.codex/decodex/projects/<service-id>/`, and `docs/spec/` outrank runbook,
  reference, and decision material.
- Keep the public site static by default. `site/` must not depend on a live Decodex
  daemon unless a later decision changes that boundary.
- Start each document with a short purpose header that says what the document is for,
  when to read it, and what it does not cover.
- Keep links explicit and stable.
- Treat `docs/research/` as a Markdown OKF research concept lane, not as a primary
  authority lane or JSON event-log write target.
- Treat promotion as an OKF/LLM Wiki maintenance event: update owner concepts,
  statuses, indexes, and links so superseded research does not outrank authority.
- Treat Decodex research output as latent until accepted or promoted through the
  loop-runtime contract in `docs/spec/loop-runtime.md`.

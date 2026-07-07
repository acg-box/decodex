---
type: "Decision"
title: "Project Autonomy Control Plane"
description: "Records why Decodex autonomy is objective-driven, project-general, and not a hidden self-repair loop."
status: active
authority: rationale
owner: runtime
tags: [decision, autonomy, objective, mcp, agents]
source_refs:
  - https://arxiv.org/abs/2303.11366
  - https://arxiv.org/abs/2305.16291
  - https://arxiv.org/abs/2303.17651
  - https://arxiv.org/abs/2405.15793
  - https://arxiv.org/abs/2407.01489
  - https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/
  - https://modelcontextprotocol.io/specification/2025-11-25
  - https://developers.openai.com/codex/learn/best-practices
code_refs:
  - apps/decodex/src/loop_contract.rs
  - apps/decodex/src/mcp.rs
  - apps/decodex/src/config.rs
  - apps/decodex/src/program_intake.rs
  - apps/decodex/src/state/store.rs
related:
  - ../spec/autonomy-control-plane.md
  - ../runbook/autonomy-implementation-roadmap.md
  - ../spec/loop-runtime.md
  - ./mcp-capability-gateway-and-skill-slimming.md
  - ./natural-language-loop-runtime.md
drift_watch:
  - Objective Contract
  - decodex.autonomy_objective/1
  - decodex.autonomy_signal/1
  - decodex.autonomy_proposal/1
  - allowed_signal_kinds
  - Program Intake
  - decodex mcp serve
last_verified: 2026-06-22
---
# Project Autonomy Control Plane

Status: decision_ready
Date: 2026-06-22
Question: Should Decodex add autonomy, and if so how does it avoid becoming an
impure Decodex-only self-debugger or an open-ended product manager?
Decision: Build autonomy into Decodex as an objective-driven, project-general
control-plane layer. Codex is the default conversational authoring UI. Decodex
server owns Objective Contracts, signals, proposals, Decision Contracts, Program
Intake, execution evidence, and authority checks. Runtime anomaly repair becomes one
`runtime_health` signal adapter, not the autonomy product.

## Decision Contract Snapshot

Selected option: Decodex as a generic project autonomy control plane.

Product form:

```text
Codex conversation / external agent / CI bot
  -> Decodex skills and MCP resources, prompts, tools
  -> Decodex server Objective Contracts, signals, proposals, decisions, evidence
  -> normal Decodex execution through Program Intake, issues, validation, review, land
```

Non-goals:

- Do not keep autonomy as Decodex-runtime-health-only repair.
- Do not add Hermes or another outer scheduler that duplicates Decodex authority.
- Do not use Codex automations as the durable project authority.
- Do not let chat history, memory retrieval, external-agent output, or MCP access
  grant execution authority.
- Do not build a standalone memory, CRM, or reporting product.
- Do not expose broad one-tool-per-command MCP mutation surfaces.
- Do not add permanent compatibility shims for the rejected anomaly-only design.

## Evidence Ledger

| Kind | Evidence | Source |
| --- | --- | --- |
| `external_source` | Reflexion shows value in task feedback and episodic verbal memory. Decodex should preserve feedback as structured evidence instead of relying on hidden model state. | [Reflexion](https://arxiv.org/abs/2303.11366) |
| `external_source` | Voyager combines automatic curriculum, reusable skills, execution feedback, and self-verification. Decodex should pair objectives, reusable agent workflows, signal feedback, and validation gates. | [Voyager](https://arxiv.org/abs/2305.16291) |
| `external_source` | Self-Refine separates generation, feedback, and refinement. Decodex should make feedback and refinement explicit proposal states rather than direct self-mutation. | [Self-Refine](https://arxiv.org/abs/2303.17651) |
| `external_source` | SWE-agent argues that the agent-computer interface materially affects software-engineering agent performance. Decodex MCP and skills should be stable agent interfaces, not incidental CLI wrappers. | [SWE-agent](https://arxiv.org/abs/2405.15793) |
| `external_source` | Agentless shows that simpler interpretable localization, repair, and validation can compete with complex agents. Decodex should keep autonomy small, auditable, and evaluator-driven. | [Agentless](https://arxiv.org/abs/2407.01489) |
| `external_source` | AlphaEvolve pairs LLM proposal generation with automated evaluators and selection. Decodex autonomy should require objective metrics and validation gates before claiming improvement. | [AlphaEvolve](https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/) |
| `external_source` | MCP separates resources, prompts, and tools and keeps tool invocation as an explicit capability surface. Decodex should expose objective, signal, proposal, and evidence capabilities while keeping authority checks server-side. | [MCP specification](https://modelcontextprotocol.io/specification/2025-11-25) |
| `external_source` | Codex guidance emphasizes goal, context, constraints, and done criteria. Codex should author the objective, while Decodex persists the resulting authority. | [Codex best practices](https://developers.openai.com/codex/learn/best-practices) |
| `repo_source` | Decodex routes execution through accepted Decision Contracts, Program Intake, validation, review, and landing. | [`../spec/loop-runtime.md`](../spec/loop-runtime.md) |
| `repo_source` | Decodex already has a typed MCP gateway and thin skill policy boundary. | [`./mcp-capability-gateway-and-skill-slimming.md`](./mcp-capability-gateway-and-skill-slimming.md) |
| `repo_source` | Current review orchestration classifies review findings through `finding_routes`; autonomy must consume that route evidence instead of raw comments. | [`../spec/review-orchestration.md`](../spec/review-orchestration.md) |

## Options

| Option | Decision | Reason |
| --- | --- | --- |
| Runtime anomaly repair only | Rejected | It explains stale lanes and protocol drift, but it cannot tell arbitrary projects what improvement means. |
| Hermes as outer manager | Rejected | It would duplicate objective, scheduling, tracker, and evidence authority already owned by Decodex. |
| Codex automations only | Rejected | Codex is the simplest UI, but thread-local automation would fragment durable authority and evidence. |
| Full Decodex memory product | Rejected | Autonomy needs a ledger and optional read-only retrieval adapters, not a second memory system. |
| Objective-driven Decodex autonomy | Selected | It keeps natural-language UX, durable authority, agent interoperability, evidence traceability, and existing execution gates. |

## Purity Decision

Autonomy is compatible with Decodex only when it remains a control-plane layer.
Decodex owns authority, orchestration, evidence, and lifecycle transitions. It does
not own arbitrary product judgment.

The purity boundary is:

- Decodex may ask "given this accepted objective and these signals, is there a
  bounded proposal worth reviewing?"
- Decodex may not ask "what should this product become?" without an accepted
  objective, non-goals, metrics, and allowed surfaces.
- Decodex may create evidence and proposal candidates.
- Decodex may not execute from a signal or report.
- Decodex may materialize accepted work into normal Program Intake or issue lanes.
- Decodex may not replace the team backlog, PR review, validation gate, landing
  authority, release gate, or installed-runtime lifecycle.

This keeps Decodex more pure, not less: the human feedback and monitoring loop that
previously lived informally in repeated chats becomes typed, inspectable, and routed
through existing Decodex authority.

## Component Decisions

| Component | Decision |
| --- | --- |
| Conversation entrypoint | Codex conversation is the default authoring UI. Other agents enter through MCP and skills. |
| Objective authority | Objective Contracts are first-class project-level authority above Decision Contracts. |
| Signal intake | User feedback, review feedback, validation failures, telemetry, spec drift, protocol drift, runtime health, and metric regression become typed signals with provenance. |
| Proposal compiler | Proposals bind objective version, signal cluster, allowed surfaces, non-goals, validation gates, evidence classes, contradictions, and gaps. |
| Execution | Existing Program Intake, issue lanes, validation, review, PR, `decodex land`, install/restart, and closeout remain the only execution path. |
| MCP and skills | MCP is the typed machine interface. Skills are the policy pack that decides when to use it and when to stop. |
| Memory | Core ledger stays in Decodex. External memory and vector systems are optional read-only adapters and cannot grant authority. |
| Reports | Reports are disposable query views over primary evidence, with source refs, redaction, completeness, and known gaps. |
| Review signals | Review-derived signals consume `finding_routes`, review-policy state, and current-head evidence. |
| Skeptic | Non-trivial proposal, architecture, repair, and decision-ready claims use fresh dynamic read-only skeptic subagents when tooling and workflow allow it. |

## Skeptic Review

Read-only skeptic review raised these objections and resolutions:

| Objection | Resolution |
| --- | --- |
| The target design had no current authority doc. | Added [`../spec/autonomy-control-plane.md`](../spec/autonomy-control-plane.md), this decision, and the roadmap runbook, then indexed them. |
| Objective Contract did not fit existing Decision Contract schema. | Defined Objective Contract as a new project-level authority above Decision Contract; mapping into Decision Contract happens only after proposal acceptance. |
| Signal and proposal pipeline was not implemented. | The spec defines `decodex.autonomy_signal/1`, `decodex.autonomy_proposal/1`, states, trust classes, fingerprints, and refusals before Program Intake. |
| Decodex could become a generic backlog/product manager. | The purity boundary states Decodex owns control-plane authority and evidence, not open-ended product judgment; executable proposals become normal issues with cold-start briefs. |
| MCP could become too permissive for "any agent." | The spec includes an action matrix separating read, submit, draft, accept, promote, intake, dispatch, lane control, and land authority. |
| Memory risk was under-specified. | The spec requires memory-derived signals to carry source refs, freshness, confidence, and proposal-only status until accepted by authority. |
| Roadmap order could start with the wrong slice. | The roadmap sequence starts with docs/decision, then Objective Contract, read-only signal ledger, proposal dry-run, acceptance/promotion, Program Intake, operator readback, then MCP mutating tools. |
| Project config had no objective home. | The spec requires runtime-owned objective records, with project config only referencing accepted objective and policy objects. |

## Consequences

- The old anomaly-only branch should not be rebased into the final implementation.
- The implementation should start from current `main` and port only the accepted
  docs/design intent.
- Existing `allowed_anomaly_kinds` style code and config should be deleted when the
  new implementation starts; do not keep compatibility aliases unless an external
  migration explicitly requires them.
- Runtime health becomes the first dogfood signal adapter for Decodex itself.
- Review routing and compact review cost-control are signal sources only; they do not
  weaken review gates or skip current-head review checkpoints.
- Codex remains the simplest human entrypoint, but Decodex server owns durable
  objective, proposal, and execution authority.
- External agents can interoperate through MCP and skills without learning internal
  DB, graph, or lane mechanics.

# Adaptive Program And Extension Architecture

Status: accepted product direction; Repeatable Program Loop V1 and the built-in Domain
Pack pressure test are implemented.

Date: 2026-08-15.

This decision guides factory work above the local Quick Task foundation. The bounded
Repeatable Program Loop V1 is current product capability. Dynamic multi-agent work,
automation, consequential external actions, and an extension runtime remain target
direction only. Current implemented behavior is defined by the
[OpenWiki quickstart](../quickstart.md), the
[local product V1 contract](../specs/local-product-v1.md), and the
[Repeatable Program Loop V1 evidence](../evidence/repeatable-program-loop-v1.md). The
two-domain result is recorded in the
[Built-in Domain Pack Pressure Test V1 evidence](../evidence/builtin-domain-pack-pressure-test-v1.md).

## Decision

Decodex will become a domain-neutral adaptive Program system above Codex app-server.
Codex remains the first cognition and worker engine. Decodex owns durable responsibility,
semantic work state, coordination, resource policy, external-effect safety, evidence,
and operator control across threads and time.

Software development is the first product domain. Investment research, scientific
research, operations, publishing, and other domains are architecture tests and possible
future extensions. They are not reasons to widen the first implementation milestone.

The target product can be described as:

> A semantic control plane that turns bounded agents and tools into a visible,
> recoverable, evidence-driven adaptive factory.

Decodex includes orchestration, but orchestration alone is not the product. The product
must also answer why work exists, what changed, whether progress occurred, which action
is authorized, and what should happen next.

## Current baseline

The current supported base is the local multi-turn Quick Task path. SQLite owns product
state, `decodexd` owns mutations, Codex app-server owns one worker thread, and
ProcessGeneration plus ProviderAttempt preserve restart and uncertain-effect safety.

Adaptive Factory Spine V1 adds one persisted Program, Signal, Claim, non-executable
Proposal, finite Objective, WorkItem, two Evidence records, and classified Review. It
binds the WorkItem to an ordinary Quick Task and derives a synchronized GPUI causal
graph and timeline. The general WorkItem board, ManagedRun, automation, dynamic agents,
general ontology tooling, and extensions remain deferred.

Repeatable Program Loop V1 keeps the Program identity and prior cycle records. After an
exact terminal Review, an operator can append one finite next Signal, Claim, Proposal,
Objective, and WorkItem. The next WorkItem uses the same Quick Task and provider-safety
path. Three real cycles now prove this sequential feedback loop. They do not prove an
extension contract, automatic stewardship, or a benefit from multiple agents.

Implementation must extend this current base. It must not copy the unlanded historical
PostgreSQL factory branch or reactivate the frozen `apps/decodex` runtime.

## First principles

1. **Durable state, episodic cognition.** Programs, facts, policies, decisions, and
   evidence persist. Model invocations wake for bounded work and then stop. No immortal
   model process or unbounded prompt is an authority source.
2. **No observation means no progress claim.** A Program can have no terminal KPI and
   no completion date. It must still define observable signals, evidence expectations,
   review triggers, resource limits, and stop conditions.
3. **Belief and action are separate.** A Signal is an observation. A Claim or Hypothesis
   is a revisable belief. A Proposal recommends an action. None of them proves that an
   external action occurred.
4. **The kernel owns effects.** Agents may reason, propose, compare, and execute through
   granted capabilities. They cannot grant themselves authority, change their own risk
   envelope, or convert an uncertain effect into success.
5. **One agent is the default.** More agents are used only for real parallel work,
   independent comparison, or independent review. Agent count is not a product setting
   or a measure of progress.
6. **The graph explains authority and causality.** It is a projection of accepted facts.
   It is not a second scheduler, a free-form canvas, or an alternate database.
7. **Extensions add vocabulary and adapters, not alternate kernels.** They cannot write
   product storage directly, replace lifecycle rules, or inject unrestricted code into
   the desktop process.
8. **A no-op can be correct.** The factory may conclude that no material work is needed.
   Continuous activity is not a success condition.

## Core semantic model

The domain-neutral kernel owns the following concepts:

| Concept | Meaning |
| --- | --- |
| Program | An open-ended responsibility, mandate, or direction. It is not an agent and has no required terminal state. |
| Signal | A sourced observation with time, freshness, confidence, gaps, and contradictions. |
| Claim | A versioned, revisable statement about the world. Hypothesis and Thesis are domain labels for a Claim. |
| Proposal | A non-executable recommendation that links Claims, alternatives, expected effects, risks, and evidence needs. |
| Objective | A finite outcome selected inside a Program. It can be achieved, abandoned, paused, blocked, or superseded. |
| WorkItem | One bounded unit of work that contributes to an Objective. |
| Run | One execution attempt by a selected worker engine under exact context, policy, capability, and resource bindings. |
| Artifact | A retained result or reference produced or consumed by work. |
| Evidence | A sourced observation used to validate, contradict, or qualify a Claim, Proposal, Run, or result. |
| Decision | An accepted choice with exact inputs, authority, rationale, and revision. |
| ActionIntent | A proposed external side effect before authorization and execution. |
| Policy | Versioned authority over allowed data, tools, resources, effects, budgets, validation, and escalation. |
| Receipt | Positive provider or kernel evidence for an exact command or effect. |

`Repository`, `PullRequest`, `Asset`, `Position`, `Order`, `Paper`, and `Experiment`
are domain concepts. They do not belong in the kernel.

### Program contract

A Program does not need one scalar KPI. It must define enough structure to review its
direction:

- purpose and desired direction;
- scope and non-goals;
- allowed Signal sources;
- evidence and freshness requirements;
- qualitative rubrics or quantitative measures when useful;
- review cadence and event triggers;
- time, token, account, money, and concurrency budgets where applicable;
- allowed actions and required authority;
- escalation, pause, and retirement conditions; and
- a current policy revision.

The system must not infer progress from message count, token use, worker count, elapsed
time, or generated artifacts alone.

### Review outcome

Each Program review records one evidence-backed classification:

- `outcome_progress`: an external or user-visible result improved;
- `knowledge_progress`: material uncertainty decreased;
- `capability_progress`: a reusable ability or validation mechanism improved;
- `no_material_change`: the cycle produced no material delta;
- `regression`: evidence shows that the state became worse; or
- `unknown`: evidence is missing, stale, ambiguous, or contradictory.

The review preserves contradictions. It must not collapse all evidence into one score.

## Feedback loop

The canonical loop is:

```text
Program charter
-> deterministic or scheduled observation
-> Signal
-> Claim or Hypothesis
-> comparison and challenge
-> non-executable Proposal
-> accepted finite Objective
-> one or more WorkItems
-> bounded Runs
-> Artifacts and external Evidence
-> Program Review
-> continue, revise, pause, escalate, or retire
```

The durable Program is not an always-running Manager model. A Program Steward is a role
that can be fulfilled by bounded Codex invocations. Deterministic services own clocks,
event subscriptions, deduplication, readiness, budgets, policy checks, attempts,
reconciliation, and stop conditions.

Observer, Comparator, Planner, Executor, and Reviewer are assignment roles. They are not
fixed product personas. One Codex thread may fulfill several roles for tightly coupled
work. Separate threads are useful only when work is independent, parallel, or needs an
independent challenge.

## Graph and ontology boundary

The graph is a typed projection over stable identities and accepted records in the
normal SQLite authority. A graph database, RDF runtime, or event-sourced replacement is
not required.

The first relation vocabulary should stay small. It may include:

```text
observes
supports
contradicts
justifies
proposes
decomposes_to
executes
uses
produces
validates
blocks
supersedes
```

Core types and relations are closed and versioned. Extensions may add namespaced types
and relations. They cannot replace the meaning of core lifecycle or authority edges.

Agents, threads, processes, and provider attempts belong to a runtime lens. They are not
the primary work graph. One WorkItem can have multiple attempts, and one agent role can
be fulfilled by different Codex threads over time.

## Extension architecture

The user-facing term is **Extension**. **Domain Pack** and **Connector** are specific
extension kinds. The existing repository `plugins/decodex` package remains a Codex
plugin and must not be confused with the Decodex product extension system.

### Domain Pack

A Domain Pack is declarative by default. It may provide:

- namespaced entity and relation schemas;
- Program, Signal, Claim, Objective, and review templates;
- role and context templates;
- declared actions and requested capabilities;
- validation and evaluator contracts;
- graph, timeline, table, card, inspector, form, and metric projections; and
- domain vocabulary and help text.

Examples include `dev.repository`, `finance.asset`, and `science.paper`. A Domain Pack
cannot execute raw SQL, alter core tables, replace the scheduler, start untracked Codex
threads, read secret values, or install another extension.

### MCP Connector

An MCP Connector supplies external data, tools, or actions. MCP is a transport and typed
adapter. It is not durable state, policy authority, or proof that an effect occurred.

The generic action route is:

```text
Agent output
-> ActionIntent
-> kernel policy decision
-> exact short-lived capability receipt
-> MCP tool call
-> external provider
-> provider readback or reconciliation
-> Receipt and Evidence
```

Limits written only in a prompt or tool description are not controls. The kernel,
connector, and provider-native account policy must enforce consequential limits outside
the model where possible.

### Programmatic extension

Custom executable extensions are deferred until a real Domain Pack cannot meet its
obligations through declarations and MCP. If required, the preferred direction is a
versioned WebAssembly Component Model host with a small capability API. Do not load
third-party native Rust dynamic libraries into `decodexd` or GPUI.

The first extension contract must not add Wasmtime, WIT, a registry, an extension store,
or a public SDK only to reserve future flexibility. Two real built-in Domain Packs must
first prove the shared abstraction.

### UI extension boundary

GPUI remains the owner of visual rendering, focus, accessibility, animation, material,
and interaction quality. Extensions compose host-rendered view primitives. They do not
inject arbitrary GPUI code.

Initial primitives may include:

- cards and compact status rows;
- tables and lists;
- synchronized graph and timeline views;
- evidence and causal inspectors;
- forms and typed action controls;
- metric, quota, and risk panels; and
- a domain-aware conversation context panel.

This keeps all domains consistent with the native desktop design while allowing useful
domain-specific views.

## External effect protocol

All consequential extension actions use the same generic attempt boundary:

```text
Prepared -> Started -> Confirmed
                   \-> Unknown -> Reconcile -> Confirmed or NotObserved
```

- `Prepared` is pre-effect and retry-safe only for the same exact digest.
- `Started` means that the provider may have received the request.
- `Confirmed` requires positive provider or kernel evidence.
- `Unknown` stops blind replay.
- `Reconcile` uses a stable provider identity or idempotency key.
- `NotObserved` permits a new attempt only when the provider contract makes absence
  conclusive enough for that effect type.

Every action binds the Program, Objective, WorkItem, policy revision, capability,
provider, resource, request digest, and idempotency identity. This generalizes the
current [ProviderAttempt authority](../specs/provider-attempt-authority.md) instead of
creating one retry model per domain.

For a broker, the connector uses an exact client order identity and provider order
readback. For source control, it uses an exact commit or pull-request identity. For a
laboratory service, it uses the provider's exact job identity. The model never decides
that a timeout means failure.

## Bounded self-improvement

The factory may generate, compare, and test candidate changes to:

- Claims and hypotheses;
- plans and decomposition;
- role selection and dynamic worker topology;
- prompts and context selection;
- strategy or workflow parameters inside an accepted search space; and
- candidate evaluators and validation methods.

The factory cannot promote its own authority. An Agent or Extension cannot change its
resource limits, secret access, real-money status, allowed providers, policy owner,
evaluator authority, extension installation rights, or kill switch.

Candidate promotion should use an explicit sequence appropriate to the domain. A common
sequence is offline evaluation, held-out evaluation, simulation or shadow mode, bounded
canary, and production. The kernel owns the promotion record and rollback target.

Program-level policy can authorize ordinary actions without one approval prompt per
step. A material expansion of scope, budget, irreversibility, or authority requires a
new accepted policy revision.

## Domain pressure tests

The architecture must support different domains without putting their nouns in the
kernel.

| Domain | Example Program | Domain concepts | Consequential action |
| --- | --- | --- | --- |
| Software development | Keep Decodex reliable and easy to operate across many Codex threads. | Repository, Change, Test, PullRequest | Commit, land, deploy |
| Investment research | Maintain a current thesis for an asset universe under an accepted risk mandate. | Asset, Thesis, Scenario, Portfolio, Position, Order | Paper order or bounded live order |
| Scientific research | Reduce uncertainty about one research question through reproducible evidence. | Paper, Dataset, Hypothesis, Experiment | Submit compute or laboratory work |

Investment is an architecture test, not a promise of autonomous profitable trading.
Profit alone is noisy evidence because market exposure, regime, chance, leakage, costs,
and risk can dominate the result. A future Investment Pack must separate decision
quality, prediction calibration, attribution, and policy compliance from raw return.
It starts with research and paper execution. Real-money capability requires a separate
accepted policy and provider-specific safeguards.

## Delivery sequence

### Milestone 1: Adaptive Factory Spine and Repeatable Program Loop V1 — implemented

Prove one real closed Program cycle through the existing Codex Quick Task path:

1. Create one Program with a charter and review policy.
2. Record one real sourced Signal.
3. Create one Claim and one non-executable Proposal.
4. Accept one finite Objective.
5. Create one WorkItem, or two only when the work is genuinely parallel.
6. Execute through existing Codex app-server and ProviderAttempt owners.
7. Collect deterministic validation and external Evidence.
8. Record one Program Review classification and rationale.
9. Show the complete causal path in GPUI with synchronized graph, timeline, evidence,
   and conversation navigation.

This milestone does not add a public extension system, arbitrary workflow builder,
automatic endless loop, graph database, cross-project scheduler, or real-money action.

The delivered V1 uses one aggregate creation command, one optional WorkItem cause on the
ordinary Quick Task command, one aggregate Review command, and two Program queries. It
does not expose a mutation API for each ontology noun. This keeps the first semantic
spine small and makes partial chain construction impossible.

The repeatable increment adds one `ContinueProgram` aggregate command. It requires the
exact terminal predecessor Review and expected Program revision. It appends one complete
pre-execution cycle, permits at most one unreviewed cycle, and keeps the same Program.
Two additional real provider-backed cycles completed after the original cycle. The
total ProviderAttempt count increased by exactly two, and a daemon restart did not add
an entity, Conversation, or attempt.

### Milestone 2: Built-in Domain Pack Pressure Test V1 — implemented

Extract only the internal vocabulary proven by two built-in fixtures:

- the Development Pack uses the real Decodex dogfood cycle; and
- a Paper Investment Pack uses public or provided data and simulation only.

The delivered slice adds exact namespaced entity and relation declarations, immutable
Pack versions and digests, deny-by-default declared capabilities, stable derived entity
IDs, host-rendered graph and inspector primitives, and capability inspection. One
additive SQLite table stores only the immutable Program binding. The domain graph remains
a read projection of the normal Program authority.

The Development Pack projects the real three-cycle Decodex dogfood Program. The Paper
Investment Pack uses one frozen official U.S. Treasury June 2025 fixture and completed
one real paper-only Program through the ordinary Quick Task and ProviderAttempt path.
Daemon restart retained both exact Pack identities, all seven derived domain entity
identities, and the exact attempt count.

This result supports one shared internal declarative Pack contract. It does not prove a
public SDK, third-party compatibility, installation or upgrade UX, a registry, an
arbitrary executable plugin host, or a dynamic multi-agent runtime. Keep Packs built in
until a concrete third consumer or external authoring need justifies a public contract.

### Milestone 3: MCP Action Gateway

Allow installed Connectors to register typed data and action capabilities. Add exact
policy receipts, provider identities, reconciliation, audit views, revocation, expiry,
and a kill switch. Start with a reversible or simulated action before a hard-to-reverse
provider effect.

### Milestone 4: Optional programmatic extensions

Add a WebAssembly host only when a concrete accepted extension requires local custom
computation that cannot remain declarative or out of process. Define the smallest API
from that consumer. Do not expose database or scheduler internals.

## Product surface

The first control room should expose:

- Program pulse: purpose, policy, attention, last review, and current direction;
- Signal inbox: fresh, stale, contradictory, and missing observations;
- Objective and Work graph;
- parallel Run timeline;
- Evidence and `why` inspector;
- Manager conversation bound to the selected semantic context;
- resource and account capacity; and
- exact authorized, blocked, or unavailable actions.

The graph is not the only view. List, card, timeline, and conversation projections must
agree on the same identities and state.

## Supersession and retained foundations

This decision changes the product context of the historical natural-language loop and
project-autonomy design:

- a visible semantic graph is now a primary control-room surface, not only a hidden
  runtime implementation detail;
- Decodex may own open-ended Program stewardship, but not an immortal Manager model or
  hidden self-authority;
- Linear, Program Intake, issue lanes, and the frozen tracker workflow are not required
  for the new local product path; and
- external MCP and extension actions must pass the new local kernel authority instead
  of an old tracker ceremony.

The following safety ideas remain valid:

- Signals do not execute actions;
- Proposals remain non-executable until accepted policy permits promotion;
- provenance, freshness, contradictions, rollback, validation, and non-goals remain
  first-class;
- deterministic code owns consequential effects and unknown outcomes;
- clients and extensions do not bypass daemon authority; and
- evidence cannot be fabricated from chat text or process exit.

This decision does not revive the frozen `apps/decodex` runtime, PostgreSQL, Linear,
redb product authority, or unlanded historical factory worktrees. The bundled SQLite
local product and current Quick Task safety harness remain the implementation base.

## Explicit non-goals

- A competing foundation model or second coding-agent kernel.
- A generic project-management or Kanban product.
- A fixed cast of permanent Manager, Observer, Worker, and Reviewer model processes.
- A product-wide fixed worker count.
- An empty-canvas workflow editor before one real closed loop works.
- A graph database, RDF/OWL runtime, or user-authored ontology language in V1.
- Arbitrary native plugins or direct third-party GPUI injection.
- Direct extension access to SQLite, credentials, scheduler state, or raw Codex auth.
- Self-installing extensions or self-expanding capability policy.
- A single reward score that can replace evidence and contradiction review.
- Automatic live financial trading in the first extension milestone.

## Evidence and uncertainty

The research supports feedback loops when external evaluators and bounded actions exist.
It does not support unlimited multi-agent collaboration or self-evaluation as a source
of truth.

- [Capable language models can outgrow the benefits of collaboration](https://www.nature.com/articles/s42256-026-01268-y)
  shows that task topology and single-agent capability can remove the benefit of more
  agents and that coordination can amplify errors.
- [Anthropic's multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)
  reports high token cost and recommends multi-agent work for high-value,
  context-heavy, parallel tasks.
- [Google's AI co-scientist](https://research.google/blog/accelerating-scientific-breakthroughs-with-an-ai-co-scientist/)
  and [AlphaEvolve](https://deepmind.google/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/)
  show iterative generation, challenge, evaluation, and selection. Their strongest
  results depend on expert or automated external validation.
- [METR time horizons](https://metr.org/time-horizons/) warns that current long-horizon
  measurements use clean, self-contained, automatically evaluated tasks and do not map
  directly to messy real jobs.
- The [MCP 2026-07-28 specification](https://modelcontextprotocol.io/specification/2026-07-28)
  keeps transport and authorization separate from host-owned durable application state.
- [Zed extensions](https://zed.dev/docs/extensions/developing-extensions) and
  [extension capabilities](https://zed.dev/docs/extensions/capabilities) support the
  declarative-first, WebAssembly, and least-capability direction. They are an
  engineering precedent, not a Decodex dependency decision.
- [KTD-Fin](https://arxiv.org/abs/2605.28359) and
  [CLQT](https://arxiv.org/abs/2606.29771) are recent preprints that expose financial
  evaluation leakage, noisy raw returns, cost, temporal gates, strategy consistency,
  and audit requirements. They do not prove deployable trading alpha.
- [FINRA's 2026 GenAI report](https://www.finra.org/rules-guidance/guidance/reports/2026-finra-annual-regulatory-oversight-report/gen-ai)
  identifies agent scope, authority, auditability, data, reward, monitoring, guardrail,
  and human-oversight risks in regulated financial use. Its rules apply to the covered
  United States firms, not every private Decodex deployment.

Confidence is high that the Program feedback loop and extension boundary differentiate
Decodex from direct Codex use. Confidence is medium on the exact ontology and UI. The
ability to judge useful open-ended progress remains unproven until repeated dogfood.

## Falsifiers and review triggers

Reduce or change this direction when repeated real cycles show any of the following:

- Program feedback does not reduce transcript polling, status collection, context
  transfer, or missed follow-up work;
- the system proposes low-value activity more often than a user working directly in
  Codex;
- graph and ontology views add more cognitive cost than list and timeline baselines;
- progress classifications cannot be supported by independent evidence;
- dynamic multi-agent execution costs more than it improves results;
- two real Domain Packs require incompatible kernel semantics;
- extension permissions cannot be understood or enforced at installation and action
  time; or
- GPUI delivery cost prevents the first real closed loop from reaching dogfood.

The first review trigger is complete: three Decodex dogfood Program cycles retained one
Program, used one provider attempt per added cycle, and produced evidence-backed
Reviews. The second review trigger is also complete: Development and Paper Investment
used the same Program, Quick Task, ProviderAttempt, capability, protocol, and GPUI host
boundaries without adding domain nouns to the kernel. This supports the small internal
declarative Pack contract. It does not validate automatic cycle creation, dynamic
multi-agent execution, public extension compatibility, or consequential external
actions. Do not design a public SDK only from these two built-in implementations.

## Curation note

This page is direct OpenWiki curation. A protected `openwiki code --update --print` run
started, then rewrote unrelated knowledge files before it stopped on worktree-state
ambiguity. Those generated changes were discarded. This page and its two navigation
updates are maintained directly and do not claim generated OpenWiki alignment.

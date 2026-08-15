# Adaptive Factory Spine V1 Goal Carrier

- Contract revision: 6
- Workspace: /Users/x/code/acg-box/decodex/.worktrees/adaptive-factory-spine-v1
- Updated: 2026-08-15T16:35:59Z
- Authority: the user request, repository `AGENTS.md`, and `openwiki/decisions/adaptive-program-extension-architecture.md`

## Objective

Deliver one real, restart-safe, closed Program cycle in Decodex. The cycle must use the
existing Codex Quick Task execution path and make the causal chain visible in GPUI:

```text
Program -> Signal -> Claim -> Proposal -> Objective -> WorkItem
        -> Codex Quick Task -> Evidence -> Program Review
```

This milestone must prove why Decodex is more useful than operating several unrelated
Codex tasks by hand. It must add coordination and explanation above Codex app-server,
not replace Codex as the worker kernel.

## Stop Conditions

The Goal is complete only when all of these conditions are true:

1. A user can create and reopen one Program with a purpose, explicit non-goals, and a
   review policy.
2. The Program can persist one sourced Signal, one Claim, one non-executable Proposal,
   one accepted finite Objective, and exactly one WorkItem.
3. The WorkItem executes through the existing Quick Task, ProcessGeneration, and
   ProviderAttempt owners. It does not introduce a second execution engine.
4. The cycle attaches deterministic validation and external Evidence to the same
   semantic identities.
5. The cycle records one Program Review with exactly one classification:
   `outcome_progress`, `knowledge_progress`, `capability_progress`,
   `no_material_change`, `regression`, or `unknown`, plus a rationale.
6. GPUI shows the complete causal path, current state, evidence, timeline, and navigation
   to the underlying Codex conversation. Card, timeline, graph, and conversation
   projections must not disagree about identity or state.
7. Daemon restart preserves the cycle and does not cause an external request to replay.
8. Focused tests, repository gates, and one real local dogfood cycle pass with durable
   evidence recorded in this carrier and the repository's accepted evidence location.
9. OpenWiki describes the delivered behavior and does not describe deferred capability
   as current capability.

## Constraints

- Pursue the smallest simple, efficient, and reliable vertical slice.
- SQLite remains the only normal product-state authority. `decodexd` remains the sole
  normal reader and writer; GPUI and CLI remain protocol-only clients.
- Codex app-server remains the worker kernel.
- Preserve conversation account binding, ProcessGeneration fencing, ProviderAttempt
  positive-evidence rules, and the no-automatic-replay boundary for unknown attempts.
- Reuse the current Quick Task implementation and current domain owners before adding
  abstractions.
- Use the stable Rust channel.
- Do not revive PostgreSQL, the frozen app, Linear intake, or an old harness as a second
  authority. Reuse a historical component only when it fits the accepted current model.
- Default to one worker and one WorkItem. Add no parallel fan-out in this Goal.
- Human approval is policy, not a mandatory prompt for every internal step. This Goal
  adds no consequential external action.
- Every positive completion claim requires current verification evidence.

## Non-Goals

- A public plugin or extension SDK, Domain Pack installation, or Wasm host.
- MCP action execution, trading, deployment, publishing, spending, or another
  consequential external action.
- A general workflow builder, graph database, RDF store, or user-authored graph editor.
- Multi-agent scheduling, dynamic worker fan-out, remote workers, or multi-machine mode.
- Cross-project orchestration, a general Kanban replacement, or a background scheduler.
- An autonomous endless loop or self-modifying optimization system.
- Rebuilding every GPUI page or changing the accepted visual language beyond what this
  vertical slice needs.

## Delivery Slices

1. Semantic spine: persist Program-cycle entities and relations in SQLite and expose
   owner-only protocol operations.
2. Execution binding: materialize exactly one WorkItem into the existing Quick Task path
   and retain its Conversation, Turn, ProcessGeneration, and ProviderAttempt lineage.
3. Review loop: attach Evidence and persist a classified Program Review with rationale.
4. Control-room projection: show one synchronized Program pulse, causal graph/timeline,
   evidence inspector, and conversation navigation in GPUI.
5. Proof and knowledge: pass restart and no-replay tests, run one real dogfood cycle,
   and align OpenWiki evidence.

## Current Checkpoint

All Goal stop conditions and local terminal gates are satisfied. Commit the reviewed
branch, land it on the authoritative default branch, verify the refreshed primary, and
launch the landed app for user testing.

## Current Ownership Map

- `crates/decodex-core/src/program.rs` owns the closed Program identities, states,
  Evidence kinds, and Review vocabulary used by V1.
- `database/` is the sole current product-state owner. Schema version 5 owns the bounded
  Program, Signal, Claim, Proposal, Objective, WorkItem binding, Evidence, and Review
  aggregate.
- `crates/decodex-runtime/src/quick_task.rs` is the mature worker path. It creates the
  Conversation before app-server effects and already preserves ProcessGeneration and
  ProviderAttempt safety.
- The general managed Project and WorkItem board commands remain typed unavailable. The
  bounded Program commands use their own aggregate owner and the ordinary Quick Task
  execution path.
- `apps/decodex-gpui/src/factory_surface.rs` renders the authoritative Program aggregate.
  Its visual-capture fixture is feature-gated and is not production authority.

The proportionate V1 mechanism is one aggregate semantic command and projection rather
than one public command per ontology noun:

1. atomically create a Program charter plus one Signal, Claim, non-executable Proposal,
   active Objective, and ready WorkItem;
2. create an ordinary Quick Task with an optional exact WorkItem cause so Conversation
   creation and the WorkItem execution binding commit in one SQLite transaction;
3. atomically record one deterministic-validation Evidence, one external Evidence, and
   one classified Program Review; and
4. query the complete causal aggregate for list, card, graph, timeline, evidence, and
   conversation projections.

## Progress And Evidence

- The accepted architecture and Milestone 1 contract are in
  `openwiki/decisions/adaptive-program-extension-architecture.md`.
- The task worktree starts from verified `main` commit
  `bb70f4aba08c7c9721742f6063e5cec105dd3a97`, equal to `origin/main` at Goal creation.
- Fresh inspection found no Program-cycle SQLite tables. Managed Factory commands are
  typed unavailable in `crates/decodex-runtime/src/application.rs`, and the current
  Factory graph remains mainly presentation-only in
  `apps/decodex-gpui/src/factory_surface.rs`.
- The historical PostgreSQL Program and WorkItem stores were inspected only as design
  donors. They are not selected implementation authority.
- SQLite migration 0005 now owns the normalized Program, Signal, Claim, Proposal,
  Objective, WorkItem, execution binding, Evidence, and Review rows. Five focused
  `program_cycles` tests pass, including atomic create, idempotency drift refusal, atomic
  Quick Task binding, evidence-backed Review, SQLite reopen, and exact command replay.
- Protocol V2.2 carries the bounded Program create/review commands and list/cycle queries.
  Its 71 library tests pass, including the Program command round-trip and duplicate
  semantic-identity refusal.
- The retained GPUI session now owns a presentation-neutral Program controller. The
  production Factory reads the authoritative cycle and renders Program pulse, causal graph,
  node inspector, causal timeline, Codex WorkItem start, Conversation navigation, Evidence,
  and classified Review. The latest focused GPUI run passes 125 tests with 2 explicit
  live-daemon tests ignored; both visual-capture targets also pass. Compilation uses the
  installed Xcode Beta Metal toolchain because the selected Command Line Tools package
  has no `metal` executable.
- The full affected-package all-target run passed every core, database, protocol,
  runtime, GPUI, and daemon target except three daemon signal tests whose macOS fixture
  path exceeded the kernel Unix-socket path bound. The fixture now uses `/private/tmp`;
  SIGINT, SIGTERM, and SIGKILL stale-socket recovery pass 3/3.
- The vNext architecture suite passes 10 tests. The staged macOS app passes strict
  codesign verification and includes the optional menubar app through the existing
  packaging flow.
- Isolated native dogfood created one persisted Program through GPUI. AX readback showed
  the selector, five pre-execution nodes, causal graph, and matching timeline. After both
  daemon and GPUI restart, a fresh session reopened the same named Program and projection.
- Real account-backed dogfood migrated a backed-up schema-3 user database to schema 5,
  created one Program in native GPUI, executed exactly one bound Codex ProviderAttempt,
  attached two Evidence records, and recorded `capability_progress`. The Objective is
  `achieved`, the WorkItem is `done`, and the Factory exposes all nine causal nodes plus
  navigation to the exact bound Conversation.
- Before and after daemon plus GPUI restart, the live database contained 43 total
  ProviderAttempts and exactly one attempt bound to this Program. The Program and
  Conversation reopened without replay.
- Dogfood exposed one stale in-memory Program projection immediately after the first
  WorkItem binding. The GPUI now retains the exact expected Conversation and invalidates
  only that selected Program after the matching Quick Task publication. Its focused
  regression test passes, and the rebuilt signed app reopens the reviewed cycle.
- OpenWiki now describes the bounded Program surface as current and keeps extensions,
  general ontology tooling, dynamic agents, and consequential actions deferred.
- The final affected-package all-target run passes every selected target. Strict Clippy
  passes with warnings denied. The schema-5 local database gate passes with 39 tables
  and five migration digests. The vNext architecture suite passes 10 tests. Changed
  Rust files pass the repository's nightly formatter configuration, and `git diff
  --check` passes.
- A final daemon signal-test failure was traced to the transport fixture inheriting an
  ambient 209 MB Codex executable. Debug startup spent about 19 seconds snapshotting and
  hashing that unrelated binary before the 20-second ready deadline. The fixture now
  owns an empty private `PATH`; all three signal cases pass in about one second and test
  only the intended socket lifecycle.

## Material Decisions

- D-001 — Implement Adaptive Factory Spine V1 as one closed Program cycle with exactly
  one WorkItem.
  - Authority: user request plus the accepted architecture decision.
  - Reason: this is the smallest end-to-end proof of Decodex's coordination value and
    avoids premature multi-agent complexity.
  - Impact: the milestone proves semantic lineage and feedback, but does not yet prove
    parallel scheduling.
- D-002 — Bind execution to the existing Quick Task, ProcessGeneration, and
  ProviderAttempt owners.
  - Authority: repository architecture and user requirement that Codex app-server is
    the bottom-layer kernel.
  - Reason: a second execution engine would duplicate mature safety boundaries and make
    the product less reliable.
  - Impact: factory semantics are added above Codex without changing the provider-attempt
    replay policy.
- D-003 — Defer extensions, consequential actions, multi-agent fan-out, and general graph
  tooling.
  - Authority: user request to prevent over-expansion and the accepted milestone order.
  - Reason: these surfaces are not required to validate the first closed feedback loop.
  - Impact: the result is a focused development-domain spine, not the final universal
    factory platform.
- D-004 — Use one atomic pre-execution Program-cycle command and one atomic
  Evidence-plus-Review command instead of exposing a mutation command for every noun.
  - Authority: agent design under the user's simple, efficient, reliable constraint and
    the accepted fixed Milestone 1 chain.
  - Reason: V1 needs the complete ontology and causality, but not a general workflow API.
    Fewer transaction boundaries also avoid partially constructed semantic chains.
  - Impact: the first UI creates one complete finite chain. Later milestones can extract
    finer mutation contracts only after real use proves they are needed.
- D-005 — Extend ordinary Quick Task creation with an optional exact WorkItem cause and
  bind it inside the existing Conversation creation transaction.
  - Authority: repository execution-safety contract and agent design.
  - Reason: a separate post-create binding command could leave a real provider request
    without durable semantic lineage after a crash.
  - Impact: ordinary Quick Tasks remain unchanged when the cause is absent; Factory work
    gains restart-safe Conversation lineage without a second execution engine.
- D-006 — Model the causal graph as normalized SQLite identities and foreign keys, then
  derive the visual graph from the aggregate query.
  - Authority: accepted architecture decision.
  - Reason: the graph must explain accepted facts and must not become another scheduler
    or data authority.
  - Impact: no graph database, RDF runtime, generic edge store, or free-form canvas is
    introduced in V1.
- D-007 — Treat `outcome_progress` and `capability_progress` as sufficient to achieve the
  finite Objective automatically. Keep `knowledge_progress`, `no_material_change`,
  `regression`, and `unknown` as closed Review facts while the Objective remains active.
  - Authority: agent design under the finite Objective and explicit progress vocabulary.
  - Reason: learning can reduce uncertainty without proving the requested outcome, and an
    unknown or negative review must not manufacture completion.
  - Impact: every run can close its WorkItem and Review, while the Program still states
    whether another finite cycle is needed.
- D-008 — Advance the exact-current local wire contract from V2.1 to V2.2.
  - Authority: repository exact-current compatibility policy.
  - Reason: Program commands, queries, events, and the optional Quick Task WorkItem cause
    change the wire shape and must never be silently sent to an older daemon.
  - Impact: client and daemon must be upgraded together; the local retained session fails
    closed on mixed versions.
- D-009 — Keep deterministic visual data behind the `visual-capture` feature only.
  - Authority: repository test-boundary policy and agent design.
  - Reason: screenshot stability is useful evidence, but fixture data must never become
    production Program authority.
  - Impact: production GPUI always reads Program state through the retained protocol.
- D-010 — Use a short macOS temporary home for daemon signal tests.
  - Authority: the kernel Unix-domain socket path bound and observed failing evidence.
  - Reason: the canonical system temporary path plus `decodex.sock.stage` can exceed the
    kernel limit before the signal behavior under test begins.
  - Impact: the fixture tests shutdown and stale-socket recovery instead of failing on
    unrelated path length; production endpoint validation remains unchanged.
- D-011 — Refresh the selected Program from the exact expected Quick Task publication.
  - Authority: real native dogfood and the protocol-owned binding transaction.
  - Reason: before the binding commits, the Program projection cannot know the new
    Conversation identity. Remembering that exact identity lets the first committed
    Quick Task publication invalidate the stale projection without polling or broad
    refreshes.
  - Impact: returning to Factory shows the Run node and settled state promptly. The rule
    remains read-only and never resends a Program or provider command.
- D-012 — Isolate daemon transport tests from the host Codex installation.
  - Authority: sampled process stack, measured startup behavior, and the signal test's
    exact transport-only purpose.
  - Reason: inheriting the workstation `PATH` made the test snapshot and hash a 209 MB
    Codex executable before it could observe transport readiness. That work is unrelated
    to SIGINT, SIGTERM, or stale-socket recovery.
  - Impact: the process-level test owns an empty private executable search path and keeps
    Quick Task capability unavailable. Production bootstrap and Codex attestation remain
    unchanged.

## Completion Report

- Delivered one persisted, restart-safe Program cycle from sourced Signal through
  classified Review, with exactly one ordinary Codex Quick Task WorkItem.
- Preserved SQLite as the only product authority and reused the existing conversation,
  routing, ProcessGeneration, ProviderAttempt, history, and positive-evidence owners.
- Delivered a retained GPUI Program controller and a synchronized Factory pulse, causal
  graph, node inspector, timeline, Evidence/Review controls, and Conversation navigation.
- Proved one real account-backed closed cycle, then restarted daemon and GPUI without
  adding a ProviderAttempt or replaying provider work.
- Preserved a pre-migration online SQLite backup at
  `/Users/x/.decodex/backups/decodex-pre-adaptive-factory-20260815T161453Z.sqlite3`.
- Verified all affected Rust targets, strict Clippy, the schema-5 database gate, the
  architecture suite, changed-file formatting, diff whitespace, signed-app staging, and
  native restart dogfood.
- Deferred extensions, dynamic multi-agent work, consequential actions, general graph
  tooling, cross-project scheduling, and endless automation exactly as declared.

## Blocker

None.

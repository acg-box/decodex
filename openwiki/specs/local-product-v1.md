---
type: "Specification"
title: "Local Product V1 Contract"
description: "Normative SQLite milestone contract for app-server freshness, Quick Task execution, Adaptive Factory Programs, lifecycle safety, and deferred capabilities."
tags: [local-product, sqlite, quick-task, adaptive-factory, domain-pack, app-server]
openwiki:
  roles: [architecture, domain, workflow]
  change_kinds: [lifecycle, public-api, runtime]
  source_paths: [crates/decodex-runtime/src/quick_task.rs, crates/decodex-runtime/src/application.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/domain_packs/decodex.dev-1.0.0.json, crates/decodex-runtime/domain_packs/decodex.paper-investment-1.0.0.json, crates/decodex-codex/src/quick_task.rs, crates/decodex-protocol/src/domain_pack.rs, database/migrations/0007_builtin_domain_pack_binding.sql, database/src/program_cycles.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/shell.rs]
  symbols: [QuickTaskExecutionSettings, QuickTaskRecoveryAction, control_thread, ExactSubmittedTurnReadback, UnknownQuickTaskAttemptReadback, TranscriptRow]
  test_paths: [database/tests/quick_task_restart.rs, database/src/program_cycles.rs, crates/decodex-protocol/src/domain_pack.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/src/application.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/client_lifecycle/tests.rs]
  invariants: [Exact thread lifecycle readback precedes local archive commit.; Missing per-thread archive fields require exact filtered-list membership.; Composer content clears only after explicit submission acceptance.; A queued prompt remains visible until durable history contains it.; Adjacent assistant fragments with the same Turn identity render as one response.; A validated fresh history head replaces the old retained page window before its continuation is rebuilt.; Unknown ProviderAttempt evidence is never replay authority.; An inconclusive Turn becomes product-usable only after positive exact process death.; A successor Context Pack excludes the successor Turn itself.; Durable terminal evidence can finish an interrupted local Turn terminalization.; Fast is request-scoped and never mutates global Codex configuration.; A live account process generation rejects a second request before provider effect.; Each Program has at most one immutable built-in Domain Pack identity.; Domain entity identities are derived from the Program and exact Pack digest.; Program capability admission precedes QuickTaskRuntime and ProviderAttempt creation.; GPUI alone renders bounded Pack projections.]
  validation_commands: [cargo test --workspace --all-targets]
---

# Local Product V1 Contract

Status: normative contract for the current SQLite milestone.

Owner: [SQLite local-product decision](../decisions/sqlite-local-product.md).

## Product boundary

`decodexd` is the sole owner of product state and external effects. Clients use the
same-UID Unix WebSocket protocol. They do not open product storage or credential files.
Codex app-server remains the provider execution kernel, with one Codex thread bound to
one Decodex RuntimeSession.

## App-server freshness boundary

Codex Desktop and Decodex are clients of Codex app-server. Codex Desktop is not a
second state authority or a synchronization peer. SQLite owns Decodex product facts,
but its projection of a bound Codex thread can become stale when another app-server
client changes that thread.

Decodex re-observes one thread at a time by exact thread ID through its bound account.
Opening a Conversation first requests its daemon-owned SQLite history. After a fresh
local page is visible, GPUI starts provider lifecycle reconciliation for the selected
thread. This ordering prevents the provider command from blocking the first local
history read on the retained protocol connection. The explicit sidebar sync builds a
bounded client-side batch and applies the same command sequentially to every local
provider-backed Conversation, then reloads the SQLite list. It does not add a bulk
provider API or a second state authority. The current V1 refresh contract covers thread
lifecycle and exact recovery of Decodex-submitted Turns. If exact app-server readback
reports that the thread is archived, one SQLite
transaction archives the Conversation and ends its active RuntimeSession. Decodex
removes that Conversation from the active task list. A Decodex archive command uses
exact pre-read, archive, and post-read in one account-bound app-server process before it
commits the same local transition. The runtime refuses refresh/archive while a turn,
establishment, or unresolved provider attempt is active, and commits the local archive
only after the exact provider result is positive and the expected Conversation and
RuntimeSession revisions still match. A prepared or dispatch-authorized active attempt
still blocks control. An `unknown` active attempt enters the Silent Recovery path below.
The sidebar reports definite refusals as skipped instead of fabricating a provider
outcome.

The local Conversation list is a SQLite read and does not assert current provider
freshness. Opening a task or explicitly syncing the sidebar requests provider readback.
V1 does not continuously poll every account and thread. App-server
`thread/read(includeTurns=true)` exposes visible but lossy history; it is not complete
history or tool-effect authority. V1 therefore does not import arbitrary external
turns during lifecycle refresh. A later history-merge contract must define identity,
completeness, provenance, and effect safety before it can update normalized history.

Current Codex app-server versions can omit `archived` from each returned Thread. A
missing field is not `false`. Decodex resolves the state through bounded, paginated
`thread/list` reads for both `archived: true` and `archived: false`, with exact thread-ID
membership and one account binding. A contradictory field, membership in neither list,
an invalid cursor, or a scan that exceeds its bound is an invalid result. No local
archive follows that result.

The sync path can repair a stale local active Turn only when exact Conversation,
RuntimeSession, and Turn revisions still match and SQLite proves that no unresolved
ProviderAttempt, non-dead ProcessGeneration, or streaming history item can own an
effect. It can archive a provider-less `starting` RuntimeSession only when no Codex
thread, thread-start fence, request, response, active Turn, ProviderAttempt, or non-dead
ProcessGeneration exists. These transitions use durable command receipts. They do not
generalize absence into proof.

Every Decodex `turn/start` sends its existing durable Turn identity as Codex
`clientUserMessageId`. This field is a correlation key, not provider idempotency. During
Silent Recovery, one same-account `thread/read(includeTurns=true)` may select exactly
one Turn that contains a user message with that client identity. A matching terminal
Turn is positive evidence: Decodex appends only an exact missing assistant suffix, records
terminal evidence, and terminalizes the original Turn without replay. If the process
stops after terminal evidence commits but before the Turn terminalization transaction,
the next sync completes that transaction from the exact durable evidence. It does not
read or dispatch the provider again. A local assistant prefix that is not an exact
prefix of the recovered text is an integrity conflict and cannot become a false
success. No match, a
nonterminal match, timeout, or read failure does not prove non-submission.

When no terminal match is available, Decodex marks the durable active Turn failed and
presents it as interrupted only after SQLite proves that its exact ProcessGeneration is
dead with positive death evidence. The original ProviderAttempt remains `unknown` and
unchanged as internal audit evidence. GPUI shows one durable “Previous turn was
interrupted. You can continue.” activity and offers automatic or explicit `Retry sync`
while reconciliation is pending. It does not ask the user to understand or discard an
`OutcomeUnknown` Conversation.

The next user message is a distinct effect. Decodex atomically ends the uncertain source
RuntimeSession, persists a bounded Context Pack, creates a new RuntimeSession on the
same account, and moves only the new active Turn to it. The new ProviderAttempt records
the exact unknown predecessor as `AcknowledgedSuccessor`. It never resumes or resends on
the uncertain Codex thread. Context compilation explicitly excludes that successor Turn,
so its user message appears only once as the final `turn/start` input. SQLite retains the
length-delimited Context Pack as authority. The runtime verifies that binary record and
renders only its represented UTF-8 sources before it sends model input.

## Quick Task execution controls

Every user send carries `QuickTaskExecutionSettings`: an explicit model, reasoning
effort, and `fast` flag. The protocol maps `fast: true` to the request-scoped Codex
`serviceTier = "priority"`; `fast: false` sends an explicit null. These settings are
part of each create or continuation request and do not mutate global Codex configuration.
The GPUI owns presentation and submits the settings; `decodexd` remains the authority
that validates the account, working directory, process fence, and provider attempt
before dispatch. A request whose account still owns a live non-dead generation is
rejected with `RestoreProcessReadiness` before provider effect rather than being
classified as acceptance ambiguity.

GPUI keeps composer content until the exact Create or Submit command reaches a terminal
accepted result. Queueing, waiting, transport loss, archived-thread rejection, and
recovery-required rejection do not clear the text. A later accepted result also cannot
clear text that the user changed after submission. If refresh removes the selected
Conversation, selection becomes empty instead of moving the draft to another thread.
Recovery commands have a separate control and never consume composer text.

GPUI projects a queued prompt into the transcript immediately and keeps that projection
until the matching durable user history item is visible. It groups adjacent assistant
message fragments with the same Turn identity into one response. A tool or system
activity row ends that response block, so text on opposite sides of an activity is not
joined. The current single-Codex view omits redundant user and assistant identity labels,
but it keeps the protocol role and Turn identities for later manager and multi-role
views. When a fresh head read succeeds, the pager replaces the old retained page window
and rebuilds its next page from the new continuation. It does not treat the old valid
successor as a continuation cycle.

Change navigation: the public settings and recovery values are in
`crates/decodex-protocol/src/quick_task.rs` (`QuickTaskExecutionSettings`,
`QuickTaskRecoveryAction`); Codex request decoding is in
`crates/decodex-codex/src/quick_task.rs`; orchestration and exact thread control are in
`crates/decodex-runtime/src/quick_task.rs` and
`crates/decodex-runtime/src/account_launch/process.rs`; GPUI submission and archive
selection are in `apps/decodex-gpui/src/quick_tasks.rs`. Focused coverage is in the
corresponding Quick Task, process reconciliation, protocol, and database tests; use
`cargo test --workspace --all-targets` only when a package or wire change crosses the
workspace boundary.

## Storage boundary

`database/` owns the fixed SQLite file, migration sequence, schema verification, store
APIs, and fixtures. The V1 schema persists:

- account identity, lifecycle operation, exact credential binding, credential payload,
  quota facts, profiles, routing control, and capability attestation;
- Conversation, Quick Task request, Turn, normalized history, and command receipts;
- routing decisions, executable continuation plans, and persisted Context Packs;
- RuntimeSession snapshots and Codex-thread establishment evidence;
- ProcessGeneration intent, exact identity, state, and positive death evidence; and
- ProviderAttempt preparation, dispatch authorization, unknown projection, and positive
  terminal evidence.

Large history content continues to use the content-addressed blob owner. SQLite stores a
bounded inline value or a digest and length.

## Repeatable Program Loop V1

The current protocol accepts exact V2.4 clients. It adds one bounded, manually repeated
Program aggregate above the existing Quick Task execution path. The initial command
creates one Program charter, one sourced Signal, one Claim, one non-executable Proposal,
one finite Objective, and one ready WorkItem in one SQLite transaction.

After one cycle has an exact terminal Review, `ContinueProgram` can append one next
Signal, Claim, non-executable Proposal, finite Objective, and ready WorkItem. The command
must carry the current positive Program revision and the exact predecessor Review. One
Review can have at most one successor Signal, and one Program can have at most one
unreviewed cycle. A stale revision, non-terminal predecessor, duplicate successor, or
parallel unreviewed cycle is rejected. An explicit next Objective marks an unresolved
prior Objective as `abandoned`; it does not rewrite prior semantic rows.

Starting the WorkItem creates an ordinary Quick Task with an exact WorkItem cause. The
Conversation and WorkItem binding commit in the same SQLite transaction. The existing
Routing Decision, RuntimeSession, ProcessGeneration, ProviderAttempt, history, and
positive-evidence owners perform the work. The Program path has no second worker engine.

A terminal Program Review command supplies one deterministic Evidence item, one external
Evidence item, one classification, and one rationale. SQLite accepts the Review only
when the exact WorkItem is running and its bound Conversation has positive terminal
ProviderAttempt evidence. The accepted classifications are `outcome_progress`,
`knowledge_progress`, `capability_progress`, `no_material_change`, `regression`, and
`unknown`.

GPUI reads the aggregate through the retained same-UID protocol. It derives the Program
pulse, causal graph, inspector, timeline, Evidence view, and Conversation navigation
from the same stable identities. It shows every retained cycle in causal order, derives
cycle numbers from Signal boundaries, marks the current cycle, and opens the exact
Conversation bound to the selected WorkItem. The graph is a projection. It is not
scheduling authority or a separate store.

## Built-in Domain Pack Pressure Test V1

Each new Program carries one required built-in Domain Pack ID. `decodexd` resolves this
ID to one exact version and manifest digest before the Program transaction starts.
SQLite schema 7 stores only this immutable Program-to-Pack identity. It does not store a
generic domain graph or a second copy of Program facts. One existing legacy Program can
receive one exact revision-fenced binding. SQLite triggers reject later update or delete.

The built-in Pack registry currently contains exactly two declarations:

- `decodex.dev` version `1.0.0` declares the `dev` namespace and derives Repository,
  Change, and Validation entities from current Program records and evidence.
- `decodex.paper-investment` version `1.0.0` declares the `finance` namespace and
  derives two Asset entities, one Thesis, and one Scenario from the embedded June 2025
  U.S. Treasury yield-curve fixture.

The runtime validates bounded namespaced entity and relation types, declared
capabilities, exact manifest digests, and the frozen fixture SHA-256. It derives each
domain entity ID from the Program ID, Pack digest, and local entity key. Therefore the
same authoritative Program read produces the same domain identities after restart.

Both Packs declare only `codex.quick_task`. Capabilities not present in the exact
manifest are denied. For a Program WorkItem, Pack admission runs before Quick Task
runtime selection. A missing binding, unknown Pack, version or digest mismatch, or
undeclared capability returns a closed error before a Conversation or ProviderAttempt
can be created. An ordinary Quick Task without a Program WorkItem keeps its existing
path.

GPUI owns all Pack rendering and interaction. It shows Pack identity, version, digest,
namespace, capability state, domain entities, domain relations, evidence, causal graph,
timeline, and the existing Conversation path with host-owned primitives. A Pack cannot
inject GPUI code, run SQL, start a thread, read a credential, or grant itself a
capability.

This pressure test does not expose a public Extension SDK, registry, store, dynamic
loader, ontology authoring language, MCP Action Gateway, live market data, paper order,
or real-money action.

## Execution invariants

1. A Routing Decision is the only initial account selector.
2. The selected account revision and exact credential binding are checked before spawn.
3. Only a fresh ProcessGeneration fence can create a child process.
4. RuntimeSession thread start is fenced before the Codex request and bound only from a
   positive response.
5. One ProviderAttempt records a consumer, plan, generation, request, and provider key
   before dispatch.
6. Only a fresh dispatch authorization can send the request.
7. Timeout, absence, process death, or restart never proves non-submission.
8. Only positive provider evidence can establish success, definitive failure, or
   non-submission.
9. Turn terminalization updates the Turn and RuntimeSession atomically with exact
   revisions.
10. Same-thread continuation requires the persisted Codex thread and exact positive
    evidence from a terminal ProviderAttempt.
11. A ProcessGeneration intent is committed before process creation. When an exact
    completed process-admission receipt exists but its target generation is absent, the
    request is positively known not to have spawned a process.
12. V1 permits one non-dead ProcessGeneration per account at one time. After positive
    provider terminal evidence and atomic Turn terminalization, the runtime retires the
    exact process and records positive death evidence before it publishes `Ready`. A
    later Turn uses a fresh ProcessGeneration to rehydrate the same account and Codex
    thread. A second request while the account still has a live generation fails before
    provider effect with `RestoreProcessReadiness`; it must not become acceptance
    ambiguity. An idle completed Turn must not reserve the account process slot.
13. Account affinity is scoped to a Conversation. Its initial Routing Decision binds the
    RuntimeSession and Codex thread to one account. Later Turns do not re-evaluate global
    routing. Independent Conversations can bind to different accounts.
14. Each user send carries an explicit model, reasoning effort, and Fast selection.
    Fast maps to the request-scoped Codex `priority` service tier. Fast off sends a null
    service tier and does not mutate global Codex configuration.
15. Provider archive readback can close a local Conversation only when no active Turn
    owns an unresolved ProviderAttempt and exact Conversation and RuntimeSession
    revisions still match. Historical unknown evidence on a closed Turn remains intact.
16. A missing per-thread `archived` field requires exact bounded membership in one
    filtered provider list. Missing or contradictory membership cannot change SQLite.
17. A stale provider-less active Turn can become failed only with exact revisions and
    proof that no unresolved attempt, live process, or streaming history owner exists.
18. `OutcomeUnknown` is not retry authority. Exact correlated terminal readback may
    terminalize the original Turn. Otherwise, positive death of its exact
    ProcessGeneration may close only the product-visible Turn while the ProviderAttempt
    remains unknown.
19. Composer content clears only for the same unchanged message after explicit command
    acceptance. Rejection and selection removal retain it.
20. Opening a Conversation must make daemon-owned local history visible before it queues
    selected-thread provider reconciliation on the same retained client connection. A
    successful fresh head read replaces the old retained page window before the next
    page is rebuilt.
21. A queued prompt remains visible until matching durable user history exists. Adjacent
    assistant fragments with the same Turn identity render as one response, while an
    intervening activity starts a new response block.
22. `clientUserMessageId` equals the existing Decodex Turn ID. It is a correlation key
    only and never authorizes replay.
23. After inconclusive recovery, a later distinct Turn uses one persisted, same-account
    Context Pack fallback and an `AcknowledgedSuccessor` attempt. It never dispatches on
    the uncertain thread.
24. A fallback Context Pack excludes its current successor Turn. Its persisted binary
    record must verify before the runtime can render represented source text for Codex.
25. Positive terminal evidence and Conversation terminalization are restart-safe. If
    only the evidence transaction committed, a later sync finishes the exact pending
    terminalization without another provider request.
26. Program continuation requires the exact terminal predecessor Review and current
    Program revision. One Review has at most one successor Signal.
27. A Program has at most one unreviewed cycle. Continuation appends one complete
    pre-execution chain atomically and never schedules it.
28. A continued WorkItem uses the ordinary Quick Task, ProcessGeneration, and
    ProviderAttempt path. Restart and command replay cannot create a second semantic
    entity, Conversation, or provider attempt.
29. A Program has at most one immutable built-in Domain Pack identity. Pack resolution
    and digest validation happen before Program creation or first legacy binding.
30. Domain entity identities derive from Program identity, exact Pack digest, and one
    local entity key. A daemon restart cannot change them or create stored duplicates.
31. Program WorkItem capability admission happens before Quick Task runtime selection.
    A rejected Pack or capability leaves the ProviderAttempt store unchanged.
32. Domain projections are bounded read models. They have no SQLite, scheduling,
    provider, credential, or visual-injection authority.

An absent or stale quota fact represents unknown capacity. Fixed routing admits an
otherwise-ready account unless a current fact proves depletion. Balanced routing prefers
known available capacity and then follows the configured order through unknown capacity.

## Restart behavior

On startup, unresolved prepared or dispatch-authorized ProviderAttempts project to
`unknown`. Nonterminal ProcessGenerations lose live supervision authority and project to
`death_unknown` unless positive evidence supports a stronger state. These projections
prevent an implicit duplicate effect.

A terminal Quick Task keeps its Conversation, history, RuntimeSession, selected account,
Codex thread, and next Turn sequence. A later user Turn can bind a SameThread continuation
after process retirement or after the daemon restarts. If the bound account becomes
depleted, V1 fails closed instead of switching the Conversation in place and losing
provider cache affinity.

A recovered unknown attempt is different from a terminal attempt. Its source
RuntimeSession remains visible until the next distinct user Turn is admitted. Planning
that Turn persists and verifies a Context Pack, ends the old RuntimeSession, and creates
a same-account starting RuntimeSession. Reopening SQLite reconstructs the same pack and
plan from migration-owned metadata plus the content-addressed blob. The current Turn is
not one of the pack sources; it remains the separate final request input.

A daemon restart also reopens every Program cycle, WorkItem binding, Evidence, and
Review. It reconstructs cycle order from predecessor Review links. Reopening or querying
a Program does not dispatch a provider request. Unknown ProviderAttempt state keeps the
existing no-automatic-replay rule.

The same restart reopens each immutable Program Pack binding. `decodexd` rejects an
unknown or digest-drifted binding instead of projecting a replacement. A valid binding
reconstructs the same bounded domain entities and relations without storing them or
creating a Conversation or ProviderAttempt.

## Credential boundary

The `account_credentials` table is physically colocated but logically narrow. General
account queries never select its payload. The daemon adapter reads or writes one exact
account record and checks schema version, monotonic credential version, fingerprint,
writer operation, provider, and provider-account binding.

The SQLite file is plaintext owner-private storage, consistent with the source Codex
authentication file and the explicit local-device threat model. No credential value can
appear in Debug output, protocol data, logs, migration output, or transfer reports.

## Deferred capabilities

The following are outside V1 and must not activate a second store:

- ManagedRepository execution;
- WorkItem board persistence;
- Reset Card consumption;
- execution-decision query projections;
- ManagedRun and automation;
- a general ontology language, graph editor, or graph database;
- dynamic multi-agent planning and worker fan-out;
- remote workers and multi-machine coordination; and
- cross-conversation app-server process multiplexing.

## Acceptance

Acceptance requires:

- a fresh installation that needs no separate database server, redb, or Keychain runtime access;
- exact database initialization, reopen, migration, inventory, and integrity checks;
- bounded idempotent transfer of the current account pool with source retention;
- one real Codex app-server response;
- a daemon restart followed by a later response on the same Conversation and Codex
  thread, with no duplicate ProviderAttempt dispatch;
- protocol-only GPUI and CLI operation;
- local-history-first Conversation opening, immediate queued-prompt projection, and
  Turn-level adjacent assistant-fragment coalescing;
- exact selected-thread refresh, verified archive, and request-scoped execution controls;
- one restart-safe Program with at least three sequential cycles, one bound Quick Task
  and classified evidence-backed Review per cycle, and synchronized causal projections;
- one Development Pack projection over the retained three-cycle dogfood Program and one
  Paper Investment Pack Program that completes through the ordinary Quick Task path;
- exact Pack and derived-entity readback after daemon restart with no duplicate
  Conversation or ProviderAttempt;
- archived-thread rejection with retained composer content and no implicit resend;
- bounded stale-local reconciliation without changing unresolved provider evidence;
- focused and workspace-wide tests; and
- current OpenWiki and local database gates.

---
type: "Specification"
title: "Local Product V1 Contract"
description: "Normative SQLite milestone contract for app-server freshness, Quick Task execution, Adaptive Factory Programs, lifecycle safety, and deferred capabilities."
tags: [local-product, sqlite, quick-task, adaptive-factory, domain-pack, app-server]
openwiki:
  roles: [architecture, domain, workflow]
  change_kinds: [lifecycle, public-api, runtime]
  source_paths: [crates/decodex-runtime/src/account_login.rs, crates/decodex-protocol/src/wire.rs, crates/decodex-protocol/src/client.rs, crates/decodex-runtime/src/account_service.rs, crates/decodex-runtime/src/auth_projection.rs, crates/decodex-runtime/src/shared_auth_coordinator.rs, crates/decodex-runtime/src/host_credentials/sqlite_store.rs, database/src/account_lifecycle.rs, database/src/desktop_settings.rs, database/migrations/0009_durable_account_route.sql, database/migrations/0010_pending_account_route_progress.sql, database/migrations/0011_desktop_settings.sql, crates/decodex-runtime/src/quick_task.rs, crates/decodex-runtime/src/application.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/domain_packs/decodex.dev-1.0.0.json, crates/decodex-runtime/domain_packs/decodex.paper-investment-1.0.0.json, crates/decodex-codex/src/quick_task.rs, crates/decodex-protocol/src/domain_pack.rs, database/migrations/0007_builtin_domain_pack_binding.sql, database/src/program_cycles.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/desktop_settings.rs, apps/decodex-gpui/src/settings_surface.rs, apps/decodex-gpui/src/shell.rs]
  symbols: [SharedAuthCoordinator, AccountService::route_account_command, recover_pending_account_routes_once, AccountRoutePendingDto, QuickTaskExecutionSettings, QuickTaskRecoveryAction, control_thread, ExactSubmittedTurnReadback, UnknownQuickTaskAttemptReadback, TranscriptRow]
  test_paths: [crates/decodex-runtime/src/shared_auth_coordinator.rs, crates/decodex-runtime/src/account_service.rs, database/src/account_lifecycle.rs, database/src/desktop_settings.rs, crates/decodex-protocol/src/wire.rs, tests/scripts/test_account_login_architecture.py, tests/scripts/test_vnext_architecture.py, crates/decodex-protocol/src/account_login.rs, crates/decodex-protocol/src/client.rs, crates/decodex-runtime/src/auth_projection.rs, database/tests/quick_task_restart.rs, database/src/program_cycles.rs, crates/decodex-protocol/src/domain_pack.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/src/application.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/desktop_settings.rs, apps/decodex-gpui/src/client_lifecycle/tests.rs]
  invariants: [Exact thread lifecycle readback precedes local archive commit.; Missing per-thread archive fields require exact filtered-list membership.; Composer content clears only after explicit submission acceptance.; A queued prompt remains visible until durable history contains it.; Adjacent assistant fragments with the same Turn identity render as one response.; A validated fresh history head replaces the old retained page window before its continuation is rebuilt.; Unknown ProviderAttempt evidence is never replay authority.; An inconclusive Turn becomes product-usable only after positive exact process death.; A successor Context Pack excludes the successor Turn itself.; Durable terminal evidence can finish an interrupted local Turn terminalization.; Fast is request-scoped and never mutates global Codex configuration.; A live account process generation rejects a second request before provider effect.; Re-enrollment restores the sole tombstoned provider owner and returns its resolved UUID.; A structured terminal device denial cannot remain pending.; Cross-account Route remains Pending until external Codex quiescence, then performs exact-source CAS/readback before routing commit.; A same-account Decodex refresh mirrors its successor or adopts a valid non-older Codex winner.; Passive shared-auth following imports only same-account non-older rotations.; AccountRoutePending is a normal credential-negative accepted handoff state.; Each Program has at most one immutable built-in Domain Pack identity.; Domain entity identities are derived from the Program and exact Pack digest.; Program capability admission precedes QuickTaskRuntime and ProviderAttempt creation.; GPUI alone renders bounded Pack projections.]
  validation_commands: [cargo test -p decodex-runtime shared_auth_coordinator::tests, cargo test -p decodex-runtime account_service::tests::live_cross_account_route_waits_for_quiescence_then_follows_same_account_rotation, cargo test -p decodex-runtime account_service::tests::projected_refresh_mirrors_success_and_adopts_a_concurrent_codex_winner, cargo test -p decodex-runtime account_service::tests::quick_task_callback_accepts_only_a_newer_same_provider_sqlite_successor, cargo test -p decodex-protocol account_route_pending, cargo test -p decodex-database pending_route_fences_balanced_and_order_revision_changes, cargo test --workspace --all-targets]
sources:
  - id: openwiki-source-98e7b23c4cc276d20fcb4649
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift
  - id: openwiki-source-35eca69c3013fea5ba400887
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/DecodexNativeCompatibility.swift
  - id: openwiki-source-a5028d07257122cad396830e
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/AccountPanelPresentationTests.swift
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-6230c010baca677fa60c32c1
    resource: repo://crates/decodex-protocol/src/client.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-268229e2b9f21dae93c32513
    resource: repo://crates/decodex-protocol/src/wire.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-82e4f987f46e34e31621081e
    resource: repo://crates/decodex-runtime/src/auth_projection.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-a9515596a887b940d069c74e
    resource: repo://tests/scripts/test_vnext_architecture.py
generated: { by: "codex", at: "2026-08-27T10:25:21.174Z" }
verified:
  - by: openwiki/0.4.2
    at: 2026-08-27T10:25:21.174Z
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

The current protocol accepts only exact 2.11 clients. It retains the bounded, manually repeated
Program aggregate above the existing Quick Task execution path. The initial command
creates one Program charter, one sourced Signal, one Claim, one non-executable Proposal,
one finite Objective, and one ready WorkItem in one SQLite transaction.

Every local 2.11 hello and welcome also carries artifact cohort `7`. The daemon, CLI,
and retained clients fail before application work when the cohort is absent or
differs. This is one compatibility fence inside the existing protocol. It does not add a
second version service or runtime authority.

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

The GPUI Accounts presentation can offer automatic browser redirect or manual device
code. Both choices use the short-lived `AccountLoginClient`; GPUI does not own a login worker. `decodexd`
owns one global `AccountLoginManager` and is the only runtime that calls the private
`decodex-account-login` provider engine. That engine is derived from official
`openai/codex` login source at peeled commit
`9392c3fa5bcda342b5b96a1a04d67b2f781617c2`, which is tag
`rust-v0.148.0-alpha.9`. It does not resolve or run a Codex executable, app-server, PTY,
terminal reader, or prompt parser.

For browser login, the provider engine binds one loopback callback on the official allowed
port and builds the PKCE/state authorize URL. GPUI opens only the transient URL. The
loopback success page reports only that browser sign-in finished and sends the user back to
Decodex for daemon installation; it does not claim that the account was added. For device
login, the provider engine requests and polls the official structured endpoints. GPUI only
copies the one-time code and opens the verification URL. Authorization and verification URLs
use an exact 8 KiB protocol bound. The dedicated Start/Status/Cancel service and all prompt
values remain daemon-memory-only; they never enter SQLite, retained snapshots/events, command
receipts, retained client caches, logs, or OpenWiki fixtures.

Both methods use the same singleton manager, owner-private temporary home, bounded HTTP
client, token exchange, mode-0600 temporary `auth.json`, timeout, cancellation, daemon-internal
Account Service installation, and exact cleanup path. Cancel returns terminal status only
after the provider worker has joined off the Tokio runtime and cleanup has completed.
`begin_shutdown` only signals; bounded shutdown waiting owns the off-runtime join. The normal
shared `~/.codex/auth.json` is unchanged. GPUI never receives a credential value or auth-file
path. The former public `EnrollAccountFromCredentialFile` and
`ReauthenticateAccountFromCredentialFile` commands are removed; private Account Service
file-import functions remain only for daemon-internal installation.

The device poll consumes a bounded nested provider error and continues only for the closed
pending-code set. A different structured 403 or 404 terminates as
`device_authorization_rejected`; the app presents one concise ChatGPT Security action instead of
waiting for the 15-minute timeout. Provider bodies and messages remain private.

Enrollment resolves the imported provider binding inside `decodexd`. If that binding belongs to
one tombstoned Account, the operation restores the original Account UUID at the exact tombstone
revision and the immediate successor credential version, then appends that UUID to routing order.
The provisional client UUID is retained only in the strict `AccountRestored` command result.
`AccountLoginClient` completion returns the daemon-resolved UUID, and GPUI refreshes that row
before reporting success. A live provider owner remains `provider_already_enrolled`. Artifact
cohort `7` fences the result shape from older local clients.

Startup also compensates the one exact pre-repair collision in which a version-one enrollment
credential reached `StoreApplied` under a provisional UUID but the provider's retained tombstone
prevented the Account insert. Only after proving the orphan target and the absence of account,
routing, quota, profile, and fixed-selection references does the daemon delete that exact
credential and cancel the old enrollment. Every other ambiguity remains recovery-required.

The provider crate records its exact upstream files and functions in its source and third-party notice.
Any upstream pin change requires a source diff of those named functions, dependency and advisory
review, deterministic browser and device parity tests, final graph/build-size measurement, and
installed-App live acceptance. A failed parity review cannot fall back to a CLI child.

The unique provider-account binding remains the duplicate-enrollment authority. If the
device-login page selects a provider identity that another Decodex account already owns,
enrollment cancels the new operation and durably records `provider_already_enrolled`.
The rejection does not expose an email address or provider identity.

The SQLite file is plaintext owner-private storage, consistent with the source Codex
authentication file and the explicit local-device threat model. No credential value can
appear in Debug output, protocol data, logs, migration output, or transfer reports.

## Daemon-owned account Route

GPUI sends one `RouteAccount` command with one Route operation UUID, the target Account UUID
and revision, the routing revision, and one idempotency key. GPUI does not run a refresh,
projection, or fixed-selection workflow. It applies the daemon's authoritative `AccountRouted`
result, including the target Account, routing control, and credential-negative projection digest,
or the accepted `AccountRoutePending` handoff for that same command.

`AccountService` holds one routing lock across Route, balanced selection, and order changes.
One daemon-wide `SharedAuthCoordinator` owns exact source reads, stable passive-following polls,
external Codex liveness observation, and the exact-source projection seam; there is no client-side
or second coordinator. A stable passive read requires two equal file-metadata observations.

Route coordinates the normal shared `~/.codex/auth.json`. If the file is absent or unmanaged, the
Route can continue with the target. A managed identity naming another enrolled Account is first
reconciled into that source Account using a stable account-scoped operation derived from the Route
operation UUID. Unknown, unreadable, unsafe, or inconsistent managed identity fails closed.

For a cross-account switch, an external Codex process can still hold and rotate the old refresh
token even after the file changes. Route therefore returns a credential-negative Pending result
while external Codex liveness is present or uncertain. It does not refresh the target, change
fixed routing, or write the file in that state. The daemon retains the original receipt and
request. Its bounded recovery loop rechecks routing and account revisions, readiness, the stable
source, and liveness. Deferring Route immediately wakes that loop. While Pending work exists, it
checks every 100 milliseconds; only the no-Pending idle cadence remains one second. A long Pending
state therefore means that conservative liveness still sees an auth-owning Codex process or
another readiness fence. It never terminates or restarts Codex.

The liveness observation identifies official ChatGPT/Codex bundle executables as strict blockers
and excludes daemon-descended app-server processes. A standalone Codex CLI is ignored only when
bounded same-UID metadata proves that its effective `CODEX_HOME` is an existing canonical home
different from normal shared auth. Unavailable or withheld home evidence stays fail-closed.
`AccountRoutePending` carries one closed `AccountRouteWaitReason`: up to eight positive PID/process
blockers with shared-or-unknown home evidence, or a typed process-observation, account-readiness,
source-stability, source-availability, or projection-readback wait. No path, environment value, or
credential crosses the protocol.

After quiescence, Route refreshes the target when required, rechecks the source, performs the
exact-source compare-and-swap, and reads the projected account back before it commits fixed routing
and the terminal receipt. The writer uses one same-directory mode-`0600` temporary file,
synchronization, atomic rename, exact readback, and parent synchronization. An already-current
target is idempotent. A client disconnect or application exit does not cancel admitted work.

For the current projected account, Decodex and Codex can both encounter token expiry. Before a
Decodex refresh, the Account Service reads the exact shared source. A valid non-older same-account
Codex rotation is imported without provider work. If the source exactly matches SQLite, Decodex can
refresh and conditionally mirror its successor. Exact readback then keeps the Decodex successor,
retries one unchanged predecessor, or imports a valid Codex winner through a deterministic
successor operation. A conflicting or older source fails closed. The losing refresh token is never
written back and winner adoption does not call the provider again.

Passive following remains separate. It imports only stable same-account non-older rotations into
daemon credentials. `AccountRoutePending`, its protocol DTO, and migrations 9 and 10 are current
handoff authority, not legacy-only decode shapes.

Focused implementation checks are in `crates/decodex-runtime/src/shared_auth_coordinator.rs`
for stable-read and exact-source coordination, `crates/decodex-runtime/src/account_service.rs` for
same-receipt pending recovery, `database/src/account_lifecycle.rs` for revision fencing, and
`crates/decodex-protocol/src/wire.rs` for pending-result validation. Run the focused tests before
the workspace suite; generated indexes and client projections are not hand-edited as part of this
contract.

The app has one Route button and applies the daemon's authoritative Pending or terminal result. It
does not own a refresh or projection workflow. While Pending, both desktop surfaces show the
current blocking PID or exact typed fallback reason and tell the user what must quit or recover.
After terminal Route, they report that the synchronized account is ready and the app can be
reopened. Account, profile, inventory, quota, and aggregate values follow authoritative revisions.
Any displayed reset-card `Total` is derived from the registry-backed account observation, not
preserved as a client-owned value across partial reads. Retryable partial account reads retain safe
prior projections and retry using bounded delays.

Route does not terminate, restart, signal, inject into, or otherwise control Codex Desktop or an
app-server process. It uses no private Codex IPC, unstable app-server injection, watcher, backup,
per-account home, or token environment projection. The file projection is authoritative for future
Codex launches and new app-server processes, not a live cross-account hot switch.

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
- protocol 2.11 with matching artifact cohort 7 across the running daemon, CLI, and GPUI app;
- one GPUI `Decodex.app` bundle whose optional menu-bar item runs in the same process and
  follows the daemon-owned `desktop_settings` revision;
- a live cross-account Route that retains Pending without changing shared auth, then completes
  the same receipt after external Codex quiescence;
- same-account refresh proof for both a successful Decodex mirror and a concurrent Codex winner,
  with no losing refresh token restored and no second provider call during adoption;
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

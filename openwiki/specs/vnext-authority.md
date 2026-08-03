# Decodex vNext Authority Contract

Status: normative target contract; implementation is gate-controlled. The XY-1403
private-artifact retirement takes effect only at the exact repository effective point
in the [retirement decision](private-artifact/decision.md#repository-effective-point).

Owner: [vNext authority decision](../decisions/vnext-authority.md). Gates:
[vNext gate manifest](vnext-gates.md).

## Product entities

| Entity | Contract |
| --- | --- |
| Project | Decodex-owned repository identity and policy; `active`, `paused`, or `archived`. |
| Agent | Stable global Advisor or exactly one stable Lead per Project; `active`, `paused`, or `retired`. |
| RoleProfile | Versioned user choice of model, reasoning effort, service tier, and instructions for `advisor`, `lead`, `task`, or `reviewer`. |
| Program | Open-ended responsibility/context; `active`, `needs_attention`, `blocked`, `paused`, or `retired`. It is not an agent. |
| Objective | Finite outcome in a Program or Project; `proposed`, `active`, `blocked`, `achieved`, or `abandoned`. |
| WorkItem | Concrete board item/execution request; `inbox`, `planned`, `ready`, `running`, `review`, `blocked`, `done`, or `canceled`. |
| Conversation | Durable logical dialogue presented by Decodex; `open` or `archived`. |
| RuntimeSession | Codex thread segment bound to an account, process, and immutable RoleProfile snapshot; `starting`, `active`, `ended`, or `diverged`. |
| ProviderAttempt | One durable external Codex turn effect bound to one Conversation Turn or ManagedRun execution, accepted runtime authorities, request identity, and positive-only outcome evidence. |
| ManagedRun | Controlled WorkItem execution with independent lifecycle, phase, and wait reason as defined below. |
| Automation | Deterministic trigger targeting a Program, WorkItem, Advisor, or Lead; `enabled`, `paused`, or `retired`. It is not an agent. |
| ContextRevision | Immutable, inspectable, provenance-linked long-term context snapshot. |
| AgentMessage | Durable sender/recipient envelope with correlation, causation, dedupe, artifact, response, and loop-budget fields; `pending`, `delivered`, `acknowledged`, or `expired`. |
| Artifact | Content-addressed large output/evidence with PostgreSQL metadata; `active`, `expired`, or `deleted`. |

There is no Domain Agent, automatic multi-Lead topology, arbitrary durable role, or Goal
product entity in V1. Task and Reviewer agents are execution-scoped. Codex-native
subagents are run-local runtime actors normalized into the same activity/message graph;
durable cross-run and cross-project routing belongs to Decodex.

Program owns an open-ended Project responsibility, its active canonical Lead owner, an
exact accepted Project Policy revision, review cadence, and bounded provenance-bearing
metrics/signals. Program context compilation is a pure deterministic data operation: it
may preserve ordinary text that mentions a conversation or thread, but it has no typed
Conversation, RuntimeSession, thread, or agent-creation field and performs no such side
effect. Objective owns a finite Project outcome, optional same-Project Program relation,
criteria, target horizon, lifecycle, and optimistic revision. `achieved` is available only
through one immutable Objective-level acceptance-and-validation record bound to the exact
Objective, Project, prior Objective revision, canonical accepting/validating Agent authority,
provenance, and chronology. That record establishes Objective outcome acceptance only; it
does not claim WorkItem or ManagedRun success. Objective abandonment remains independent.
ManagedRun may reach successful terminal completion only from explicit authoritative
WorkItem acceptance and validation. Objective achievement or evidence and any external
Codex Goal state cannot establish WorkItem acceptance or ManagedRun success.
The evidence persists the exact prior Objective `updated_at`; acceptance cannot predate that
revision, including through direct SQL. Program and Objective mutation receipts are scoped
from the caller's canonical Project authority and PostgreSQL verifies the stored Project
match, so not-found replay remains deterministic even if the identity is created later.

## Interaction and work

Advisor is the global default, owns consultation and cross-project recommendations, and
cannot modify project code or call project write tools. A Project Lead owns project
context, user decisions, WorkItems, execution-mode selection, dispatch, and result
acceptance through one serial decision queue. Task and Reviewer agents may execute in
parallel. Reviewer output never mutates code; for a managed implementation task, the
owning Task thread performs accepted repairs under the same WorkItem. Separately
dispatched non-implementation review remains an execution-scoped Reviewer run.

For a managed independent implementation task, the Task thread owns its inner quality
loop: implement, spawn an independent read-only reviewer subagent, evaluate every
finding, repair valid findings, revalidate, and hand the result back to the Lead/Manager.
The Lead/Manager owns dispatch, final acceptance, and merge. Quick Tasks are exempt. A
missing or failed reviewer cannot produce a successful reviewed run; the ManagedRun
remains `waiting` and the WorkItem may become `blocked`, with a typed
`reviewer_unavailable` or `reviewer_failed` wait reason.

A Quick Task is an ordinary multi-turn Codex conversation with no WorkItem, ManagedRun,
reviewer, PR, harness, or Goal. A ManagedRun separates:

- lifecycle: `queued`, `active`, `waiting`, `terminal`;
- phase: `prepare`, `execute`, `validate`, `review`, `repair`, `land`, `close`;
- wait reason: `usage`, `auth`, `plugin`, `dependency`, `approval`, `user`, `external`,
  `reconciliation`, `reviewer_unavailable`, `reviewer_failed`, `reviewer_ambiguous`.

Before V26, the inert V12 boundary persisted only `waiting` ManagedRuns that remained blocked.
Every run was foreign-key bound to its exact Project, canonical WorkItem, and authoritative
RuntimeSession revision. Task and Reviewer assignments remain exact-run RuntimeSession identities.
Their closed role type cannot represent Advisor or Lead and contains no durable Agent identity.
V12 effect lineage was foreign-key bound to one run and one barrier. Barrier states were `guarded`
or permanently `closed`; both denied effects.

The historical V12 mutation consumed a positively observed unknown exact turn, a Decodex-owned
exact submitted-turn receipt, or an explicit inconclusive observation. It preserved a blocked
waiting run and recorded divergence only from positive unknown-turn evidence. Missing, empty,
exhausted, not-found, scan-exhaustion, no-event, or method-result absence never authorized
progress.

V26 removes the drained V12 submitted-turn, safety-input, effect, and barrier relations and their
exact writer. The cutover stops if any live row or V12 exact-command receipt remains. It does not
create a compatibility or fallback writer. The V3 Turn and HistoryItem invoker-rights repairs that
V12 introduced remain current and independent of the retired ManagedRun-local effect authority.

V24 cuts external-turn authority over to the generic ProviderAttempt owner. V12 submitted-turn
receipts, safety inputs, and effect barriers are not inputs to ProviderAttempt submission or
outcome. V25/V26 adapt V14, V16, and V17 for both consumer paths and retire the old V12 shapes.
ProviderAttempt remains the sole external-turn attempt, receipt, and ambiguity authority.

Project/Program policy is versioned authority over allowed repositories, tools, paths, merge
behavior, parallelism, budgets, approvals, and quiet periods. Commands use expected
revisions and idempotency keys. Side effects require receipts and authoritative readback;
an outcome that may already have caused side effects is reconciled, never blindly
replayed.

## Runtime and state authority

| State/surface | Authority |
| --- | --- |
| Projects, agents, policies, Programs, Objectives, WorkItems, ManagedRuns, Automations, profiles, context, messages, mappings, and UI-visible conversation/activity projections | PostgreSQL domain tables with optimistic revisions, leases, append-only activity projection, and transactional outbox |
| Codex thread continuation and Codex UI visibility | persistent Codex rollout under the shared normal `~/.codex` |
| Repository files and worktrees | Git/filesystem on the `decodexd` host |
| PR/check/merge readback | GitHub |
| Large tool output and evidence bytes | content-addressed local blob store, with PostgreSQL metadata |
| GPUI local state | bounded disposable cache only; SQLite is permitted only here |
| Account product state | PostgreSQL Account Registry; it stores credential-negative identity, independent enabled state, observed health, routing mode/order, quota, usage/profile/history, credential-version evidence, and finite operation receipts |
| Credentials | narrow versioned HostCredentialStore; PostgreSQL and clients never store or receive credential bytes |
| v0.2 state | final vNext runtime and installer read none; an operator can copy local credentials once through ordinary vNext account imports and then delete the frozen account pool |

PostgreSQL is not event sourced and no graph database is used. Stable IDs plus correlated
activity derive graph/timeline projections. `decodexd` is the sole product scheduler,
app-server child owner, mutation coordinator, and repository-side-effect owner. GPUI,
SwiftUI menubar, CLI, and MCP are clients/adapters over common application services; they
never read PostgreSQL, rollout files, blobs, or repositories directly. V1 is single-host
and has no worker registry or distributed mesh. Remote UI may be added only through the
protocol security gate.

The account ownership, refresh, recovery, platform-store, clean-cutover, and readiness
contract is [Account Lifecycle Authority](account-lifecycle-authority.md). An account's
versioned `enabled` value is independent from observed health and quota. Enable, disable,
fixed selection, balanced selection, and account-order changes are deterministic
versioned CAS commands. The environment-backed projection and legacy account watcher are
retired and cannot be a normal Slice-1 or Slice-3 runtime dependency.

`decodexd`, its daemon-private PostgreSQL runtime identity, and its BlobStore access form one
trusted service boundary. PostgreSQL owns committed metadata, domain state, command receipts,
activity, and outbox records; local content-addressed storage owns large bytes. PostgreSQL alone
does not attest external bytes, and arbitrary/manual use of the daemon credential is unsupported
and equivalent to daemon compromise. Blob-backed commands use a durable receipt-first saga: a
committed pending receipt binds protocol, operation, project/scope/entity, request digest, expected
revision, and payload hashes/lengths before publication; sorted session hash locks and per-shard
admission serialize create-only verified publication; transaction B atomically registers
metadata/domain references/evidence, stores the exact response bytes, and completes the fenced
receipt. Exact replay returns those bytes; conflicting reuse fails before effects.

### Manual reset-card authority

Manual reset-card use is a common vNext application service. `decodexd` is its sole
credential-vault reader, Codex app-server child owner, opaque provider-credit resolver,
mutation coordinator, and external-effect owner. CLI and SwiftUI are clients. They cannot
read credentials, launch app-server for this operation, receive a provider credit ID, or
own effect retry.

The public selection contract contains one canonical vNext Account UUID, its exact
optimistic revision, and one credential-negative card descriptor made from grant and
expiry timestamps. New admission requires `enabled=true`, AccountLifecycle readiness,
no unsettled account operation, and exact agreement among the account revision,
HostCredentialStore version and fingerprint, and provider binding. Observed quota or
health does not replace these gates. Manual Reset Card use does not enable conversation
dispatch or automatic fallback.

The Codex adapter must prove that one generated schema advertises both
`account/rateLimits/read` and `account/rateLimitResetCredit/consume`. It must establish a
complete unique inventory before it maps the public descriptor to one exact opaque credit
ID. A read can publish a provider-reported available count with incomplete or absent detail
rows, but it must publish no selectable descriptors from that partial inventory and must
retain independently valid quota facts. A reported zero count is a definitive complete empty
inventory. Null quota reset timestamps are unsupported windows, not protocol corruption.
Before an observation, the Account Service refreshes only an expired access token or one
that cannot cover the bounded provider-process deadline, under the existing account lock,
refresh journal, and credential compare-and-swap. The daemon persists the resolved exact ID
and the unchanged logical-command idempotency key before it starts the provider effect. A
terminal result requires a closed provider receipt and a fresh complete authoritative
inventory readback. After an ambiguous stop, restart recovery may retry or reconcile only
the persisted exact ID with the same key. It must never rematch a new inventory item or
create a new provider key.

The caller creates and durably records the logical-command key before `use`. Account and
inventory results bind the selected profile name and stable server UUID; all later
client calls can require both values. A remote profile is not reset-card authority and
must fail before transport until authenticated remote reset-card transport exists.

The daemon persists a credential-negative account-binding fingerprint over the account
UUID and configured provider identity fields. Restart must reject drift, and generic
account mutation cannot replace or remove the binding. A terminal same-key receipt
replays unconditionally before any current account, store, provider, readiness, or
enabled-state check. New work and the pre-effect fence both require `enabled=true`,
AccountLifecycle readiness, no unsettled operation, and exact account revision,
credential version/fingerprint, provider binding, and selected oldest public descriptor.
A terminal effect-present readback erases the private exact-ID and provider-key projection while it
retains the public receipt, reconciliation status, and replay result. Generic retention
must not prune this reset-card ledger. A terminal pre-effect rejection or exhausted
`not_started` claim also erases the private projection.

An unexpired pending receipt yields acceptance unknown. An expired pending receipt is
not absence. The same exact request may reclaim it only through a row-locked claim fence
that first observes any late commit. It must replay a committed result, reclaim after
rollback, or remain unknown. A deterministic pre-effect business rejection must complete
the claimed receipt with a closed replayable rejection. A mechanical failure leaves the
receipt pending and cannot become a rejection.

Swift may persist only a bounded credential-negative recovery handle that includes the
profile and server authority. Journal corruption must be preserved and must block new
use. One cross-process critical section must cover the last persisted-handle check,
consume invocation, dispatch classification, and terminal handle removal.

<a id="private-artifact-authority"></a>

### Private artifact retirement

At and after the
[repository effective point](private-artifact/decision.md#repository-effective-point),
vNext has no private-artifact subsystem, API, runtime composition, controller,
PostgreSQL authority, executor, platform contract, garbage collector, delivery lane,
or future acceptance program. The
[private-artifact archive](private-artifact/README.md) preserves the former design as
historical evidence only. Its rule markers, inventories, dependency edges,
A0/A1/B/D0a/C/D phases, CORE-FREEZE, ACC, preparation, and unified validation are
non-executable and cannot authorize future work.

The accepted Artifact entity and BlobStore contract in this document remain
unchanged. XY-1369 and XY-1370 use bounded canonical privacy-safe Git evidence for
the exact retained-title receipts that XY-1363 consumes. That transport creates no
new product Artifact, service, schema, storage system, runtime route, platform layer,
issue, or compatibility path. Raw schema and other private or unbounded output do
not enter Git, Linear, Artifact, logs, or receipts.

XY-1373's former moving-core integration and landing condition is historical and
non-executable. Its later cancellation preserves its complete history, parent, and
`relatedTo` relations and does not claim that integration completed. The later automatic
fallback and wake paths stay disabled until the separate reviewed XY-1304 amendment.

XY-1371 and the XY-1378-XY-1391 private-artifact execution graph are also inactive
historical planning provenance. Repository authority already retired that program.
They cannot gate the delivery slices or restore a private-artifact authority.

### Managed repository authority

The accepted XY-1348 stage-two contract makes PostgreSQL the current durable authority
for each managed repository's projection, monotonic generation/tip, globally immutable
operation assignment, append-only authority transitions and operation evidence, exact
generation/tip compare-and-swap, atomic command completeness, and every restart load.
Pure value types, descriptors, transition-specific evidence, and deciders in
`decodex-core` remain mechanism-neutral and explicitly non-authoritative. They cannot
infer persistence freshness, COMMIT success, or global operation history. No snapshot,
caller-supplied projection, generic observation, operation view, or reconstructed state
can be supplied back as mutation authority.

Within the trusted single-host V1 boundary:

- `decodexd` is the sole owner of repository and worktree effects. No client, provider,
  validation child, second daemon, or distributed worker acquires a parallel mutation
  path.
- The in-process repository executor preserves correctness, deterministic decisions,
  and continuity from explicitly admitted repository authority through effect readback.
  It is not a sandbox and does not isolate the service from malicious code with the same
  host UID.
- Admission, allocation lifecycle, mutable repository/worktree head, active operation,
  and operation result are distinct typed authorities.
- Every operation fails closed on stale revisions, foreign identity, any symlinked path
  component, object or descriptor replacement, dirty state, ambiguous observation, or
  incomplete authoritative readback.
- Repository-controlled Git config and includes, hooks, filters, `fsmonitor`, credential
  helpers, askpass, SSH, and transports are disabled unless an explicit managed policy
  allowlists exact reviewed behavior. The accepted XY-1354 mechanism closes its canonical
  config, environment, executable-identity, and path-output surfaces; every omitted or
  unmatched surface remains disabled and fails closed. Ambient environment, current
  working directory, and repository discovery never grant authority.
- Project validation is supervised for process lifecycle, bounded output, timeout and
  cancellation, and repository mutation detection. Deliberately hostile same-UID code is
  outside V1 confinement. Hostile-project or multi-tenant operation requires a separate
  UID or sandbox owner and an independently accepted feasibility and authority gate.

Every external operation is assigned one complete canonical descriptor. The descriptor
contains every value capable of changing execution or success evidence, including the
operation, project, repository, admitted identity and base, admission descriptor digest,
allocation and worktree identities, persisted absolute repository and worktree paths,
expected aggregate checkpoint, operation kind, complete kind-specific payload, and
executor-contract version. Optional values have an explicit null representation; field
and collection order is canonical. Equality compares the complete canonical
representation rather than a digest. The namespace is global across repositories and
operation kinds, not per kind.

An unassigned ID may become a new assignment. Complete canonical equality with an
existing assignment resolves to `ExistingExact(OperationView, NoDispatch)`, whether the
view is `PossiblyEffected`, completed, or ambiguous. Any difference is permanent
`OperationIdConflict`. Exact repeat is immutable result/readback access only; it is never
retry, replay, adoption, or dispatch.

One top-level PostgreSQL transaction canonicalizes the new operation, resolves its global
ID, locks and loads current authority, verifies projection/checkpoint/fence agreement,
runs the pure decision, inserts the immutable assignment, appends `PossiblyEffected`,
fences allocation or head, appends the authority transition, and advances the projection
with exact generation/tip compare-and-swap. Commit-time completeness prevents any subset
from committing. Assignment and terminal evidence remain immutable and retained across
repository retirement or deletion.

The adapter may privately retain a non-executable pre-COMMIT seed. One fresh affine
receipt may be minted only when COMMIT returns successful acknowledgement on that same
live adapter control path. The receipt is neither cloneable, serializable, persistable,
queryable, nor publicly constructible. Persistence, `SELECT`, readback, exact repeat,
restart, and terminal state can never mint or reconstruct it. If COMMIT may have succeeded
but acknowledgement is lost, the invocation returns an unknown preparation outcome, no
receipt exists, and no external execution occurs. A later exact request may resolve an
existing assignment without dispatch or, if no assignment exists, perform a wholly new
preparation whose own successful COMMIT acknowledgement is the only possible receipt
source.

Allocate is PostgreSQL-only. Descriptor-assisted admission facts, symlink-free verified
persisted absolute-path reacquisition, identity/stat facts, read-only Git facts, and target
availability observations must remain strictly read-only: they create no file, directory,
lock, reservation, worktree, index, config, or Git mutation. Allocation claims the exact
repository/allocation/worktree/path identities and initial head only in PostgreSQL.

`Register`, `WorktreeReady`, and `Commit` are separate durably fenced
`PossiblyEffected` operations:

- `Register` is the accepted pinned Git 2.54 worktree-add operation. Completion
  requires exact reciprocal registration and the unchanged authorized head.
- `WorktreeReady` is a distinct registered-to-ready operation whose positive readback
  preserves the exact head.
- `Commit` consumes exact head `H` and positively reads back exactly one advance to the
  canonical successor `H-prime`.

Every restart loads PostgreSQL authority and may issue only an operation-specific,
strictly read-only readback for a committed `PossiblyEffected` operation. Positive
transition-specific evidence may complete it; authoritative negative, foreign, dirty,
rollback, replacement, or bounded inconclusive evidence may make it ambiguous; temporary
readback unavailability leaves it `PossiblyEffected`. Restart never prepares the existing
ID, reconstructs a receipt, invokes or retries the effect, replays, adopts, repairs, or
imports external state. Generic observations cannot complete any operation.

Authorized whole-cluster restore is inside the trusted PostgreSQL-administrator boundary
and may remove or resurrect assignments, checkpoints, and results together, thereby
redefining current authority. V1 has no external monotonic anchor and no automatic
full-cluster rollback detection. The accepted trusted single-daemon/same-UID boundary,
XY-1354 descriptor-assisted symlink-free persisted absolute-path reacquisition, and
pinned Git 2.54 mechanism remain unchanged.

XY-1349 solely owns V13 physical persistence, transaction mechanics, privileges,
retention, migration, and frozen database evidence. XY-1350 may proceed in parallel only
against this accepted contract and owns read-only acquisition plus executor/readback
mechanics, not persistence, receipt minting, saga, or hidden allocation mutation. XY-1351
owns the first shared path that composes preparation, fresh receipt consumption, execution,
readback, and terminal reconciliation. Rejected candidate trees
`6e20e9b3cf1415cce9b399da173b0410cc4c80dc`,
`6979e3831da772fca3fe0f0e0b4699df642d3a65`, and
`e42212add13af3f702e0ec8966ce3d6a7b682d12` are superseded evidence only. This contract
creates no compatibility or history migration path.

Pure PostgreSQL commands use a different, exact in-transaction authority. Each operation has one
command-complete migration-owner `SECURITY DEFINER` function. PostgreSQL constructs the complete
request JSONB from the same typed values the function consumes; runtime supplies only a
protocol-scoped idempotency key and typed operation inputs, never an authoritative
caller-supplied request hash, claim token, lease, committed pending claim, or split-phase reserve.
The separate `decodex.exact_command_receipts` primary key is
`(protocol_version, idempotency_key)`. Operation is inside the request envelope, so
cross-operation reuse conflicts without extending or changing legacy `command_receipts` semantics.

An exact row may be `executing` only within its operation transaction. A
`DEFERRABLE INITIALLY DEFERRED` constraint trigger rejects commit unless every newly created exact
row is completed success or completed stable rejection. Completed rows cannot be changed, deleted,
or truncated and retain the authoritative response bytes created once by PostgreSQL. Expected
missing-target, stale-revision, illegal-transition, and equivalent domain outcomes complete a
stable rejected response; cancellation, connection loss, deadlock, serialization failure, and
unexpected database failure propagate and roll back rather than becoming stable rejection.

The normal contract is one exact command per top-level `READ COMMITTED` transaction. After
`INSERT ... ON CONFLICT DO NOTHING`, replay/conflict selection occurs in a later read/lock
statement. `40001` and `40P01` retry the whole transaction with the identical typed request.
Multiple exact functions in one caller transaction remain atomic but are outside the no-deadlock
guarantee.

Request envelopes compare with JSONB equality, not containment. Every optional key is present with
JSON null. Enum and numeric values are typed before construction; integer lexical spelling is not
identity. Text uses exact PostgreSQL text/code-point semantics with no implicit Unicode, case, or
whitespace normalization. RoleProfile bootstrap takes four role-implied scalar configuration
groups in advisor/lead/task/reviewer order, never caller roles or parallel arrays. Derived
revisions, selected profile rows, generated IDs, database timestamps, digests, immutable snapshots,
activity/outbox IDs, and responses are effects rather than request inputs. Effects and stored
responses are assembled from actual `INSERT`/`UPDATE ... RETURNING` rows and actual canonical
activity/outbox identities.

Exact-command catalog closure covers the unreachable owner; role membership and `SET ROLE` paths;
signatures and overloads; `prosecdef`; language, volatility, parallel safety, settings, source and
dependencies; ACLs, PUBLIC and owner default privileges; trusted search path; triggers; relation
privileges; and populated restore. Runtime has no exact-receipt table privilege, private-helper
execution, or canonical activity/outbox mutation authority. Namespace fences must reject equivalent
aggregate/event/effect/link/payload forgery, including structured variants, rather than matching
only obvious strings.

Relation privilege closure is semantic: it enumerates the normalized grantee, grantor, privilege,
and grant-option set; proves the owner's complete effective table privileges; proves runtime and
PUBLIC lack every table privilege; and rejects any unexpected grantee before and after restore. It
must not require byte- or text-identical `relacl` serialization. Function closure covers the exact
identity and overload set of every command, private helper, envelope builder, trigger function,
failpoint, and incomplete-row probe present in the candidate, with only command-complete entrypoints
runtime-executable. Effect evidence decodes the stored response bytes and joins their effect envelope
to the returned domain row and actual canonical activity/outbox identities.

This authority is implemented vertically: XY-1345 records and proves the protocol; XY-1346
implements the separate relation and RoleProfile bootstrap/update in V9; re-bounded XY-1337 owns authoritative
RuntimeSession snapshot creation/transition in V10. Candidate 3 is superseded code and may supply
only independently re-derived invariants and hostile-test ideas.

V9 persists exactly the `advisor`, `lead`, `task`, and `reviewer` identities in
`role_profiles`, keeps every configuration in immutable `role_profile_revisions`, and advances one
current-revision pointer per role. `bootstrap_role_profiles_exact` accepts four fixed
advisor/lead/task/reviewer scalar groups and creates all four revision-one profiles atomically.
`update_role_profile_exact` accepts one typed role plus an expected revision, appends exactly one
immutable revision, and advances only that role's pointer. Both functions return and retain
PostgreSQL-built response bytes whose effects are assembled from the returned profile rows and the
actual canonical activity/outbox identities.

## Conversation, context, and communication

Every meaningful Decodex-created thread uses `ephemeral=false`, the shared normal
`~/.codex`, the repository `cwd`, and a title/provenance marker retained for supported
exact-ID and filtered-list ownership readback. Advisor and Lead threads are never
auto-archived. Task/Reviewer threads remain discoverable through that supported boundary
and are archived only by explicit retention policy; probes may be ephemeral. Decodex
never imports Codex-created threads and persists mappings only for Decodex-created
threads. No global Codex or Codex Desktop title-search/indexing contract is claimed until
the separate live enablement gate records the required supported discovery readback.

A logical Conversation may span RuntimeSessions when size, resume latency, compatibility,
or account failure requires it. Each mapping records conversation, session, Codex thread,
account, profile snapshot, and last known turn. Decodex persists normalized visible
messages/items for UI and remote access and offloads large payloads to blobs. Issued history cursors
bind both membership high-water and an append-only immutable item-version sequence, so later
streaming mutation cannot change a page or replay. Cursor chains are opaque, Conversation-bound,
fixed-page-size, one-hour expiring, and bounded to 512 rows per Conversation and 4,096 globally;
bounded pruning retains versions required by active cursors or exact receipt replay. Successful
reads verify every direct and transitive referenced blob before returning typed metadata.
The daemon preserves a canonical media type for inline and offloaded entries and exposes only a
flat credential-negative metadata map: at most 32 fields, 64-byte keys, and boolean or 256-byte
string values. Core owns this typed representation and every Rust boundary reuses it. Keys whose
ASCII-alphanumeric normalized form ends in a credential-bearing suffix are rejected, as are concrete
authorization schemes, known token/key formats, credential assignments, embedded URL passwords, and
private-key headers. Ordinary prose containing words such as `secret`, `token`, or `session` is not
credential material. PostgreSQL enforces the equivalent closed predicate. Nested/raw app-server JSON
and unsupported, oversized, or credential-shaped forms are rejected.
`thread/read(includeTurns=true)` and paginated/list evidence are lossy reconciliation sources.
They may support positive observations, but never authorize a negative `Present`, `Complete`, or
context-free `Absent` conclusion. Missing, truncated, or unobserved evidence remains unknown.
Experiment intent and the first creation fence belong to XY-1358/V15. XY-1367/V22 binds the exact
nullable-name start response. It fences the separate name-set effect. It requires exact-ID
retained-title attestation before positive observations or V17 same-thread authority. Production
Quick Task creation belongs to XY-1276. External Codex activity may be provenance-imported for ordinary
Quick/Advisor/Lead conversations; on an active ManagedRun it marks the session `diverged` and blocks
side effects until tool/repository readback reconciles them.

### XY-1276 Quick Task thread establishment

Status: Candidate 5 is approved architecture only. Implementation, behavioral commit and
exact-tree review, Phase A, digest-only child review, final mechanical preparation, Phase
B, canonical aggregate validation, landing, installation, and live verification are
pending. The rejected Candidate-4 evidence and migration allocation are in the
[authority decision](../decisions/vnext-authority.md#xy-1276-candidate-5-architecture-reset).

Quick Task remains an ordinary multi-turn Conversation. Candidate 5 uses existing owners
in this exact order:

1. The Conversation owner creates the Conversation.
2. The routing fixture supplies a prospective Turn UUID as intent identity. It does not
   create the Turn, and no routing column has a foreign key to `turns`.
3. V16 runs once for the initial operation. It loads and locks the complete V14 universe
   and selects the account. The request has no source RuntimeSession and no sticky member.
4. V17 consumes that exact selected decision. In one transaction it creates the first
   selected-account snapshot, the first selected RoleProfile snapshot, one `starting`
   RuntimeSession, one inert `initial_thread` plan, activity, outbox, and exact receipt.
5. The existing conversations owner admits the exact prospective Turn and its first history
   item in one transaction.
6. Every establishment effect fence locks and rechecks that exact Turn as active revision 1
   under the same Conversation and V17-created RuntimeSession. ProcessGeneration and
   pre-bind thread effects require the applicable `starting` revision. ProviderAttempt
   preparation and authorization require the exact post-bind `active` revision and its
   fence/bind receipts.
7. Account Service fences the exact V16-selected account immediately before spawn.
8. Only a fresh ProcessGeneration fence can spawn. V34 then owns exact thread-start fence
   and bind. ProviderAttempt owns preparation, authorization, and provider-effect state.

There is no second V16 call, fallback, wake, alternate account, or account re-selection in
this initial operation. V17 consumes the selected V16 decision and does not select an
account. Its initial planning is first-session creation. It is not explicit successor and
is not Context-Pack fallback. V17 same-thread and Context-Pack fallback keep their existing
owners and remain separate from this initial operation. XY-1304 continues to own later
automatic fallback and wake.

The owner boundary is:

| Owner | Candidate-5 authority |
| --- | --- |
| Account Registry and V14 | Complete lifecycle, control, capability, quota, and routing facts. They do not select. |
| Account Service | Account lifecycle operations and the exact selected-account readiness, credential, provider, and HostCredentialStore pre-spawn fence. It does not select the Quick Task account. |
| V16 | The sole Quick Task account selector and immutable initial route-decision writer. |
| V17 | Initial snapshots, first starting RuntimeSession, and inert initial plan; existing same-thread and Context-Pack planning; PostgreSQL-only explicit-successor evidence. |
| Conversation owner | Conversation creation, atomic initial Turn/history admission, legal Turn finalization, and exact Turn lock/read proof. |
| V34 RuntimeSession owner | RuntimeSession state/thread fields, exact thread fence and bind, acknowledgement, and the seven constrained trigger-function roll-forwards below. |
| ProcessSupervisor | ProcessGeneration intent, fresh spawn authority, exact readback, positive death evidence, and account-local quarantine. |
| ProviderAttemptService | Attempt preparation, dispatch authorization, ambiguity, positive evidence, and reconciliation. |
| ExecutionCoordinator | A crate-private stateless sequence across these owners. It stores no state and grants no authority. |

User-controlled fixed routing, balanced order, account UI, deterministic account aliases,
and account lifecycle commands remain unchanged. Account Service can report and fence the
facts that V16 selected. It cannot preselect, substitute, fall back to, or wake another
account. In particular, if account A passes an Account Service subset but fails a V14
capability and account B is fully eligible, V16 must select B. V17 creates only B's
snapshots and RuntimeSession. Later B drift fails closed before spawn without a second
decision, account A, or another fallback.

For `routing_snapshots`, the initial-lineage shape is closed and named:

- `L0` means all six lineage fields are null: `runtime_session_id`,
  `runtime_session_revision`, `account_snapshot_id`,
  `account_snapshot_source_revision`, `profile_snapshot_id`, and
  `profile_snapshot_source_revision`.
- `L6` means all six fields are present and each of
  `runtime_session_revision`, `account_snapshot_source_revision`, and
  `profile_snapshot_source_revision` is positive.

V34 may drop `NOT NULL` only from those six columns. It must preserve the existing
RuntimeSession, account-snapshot, and profile-snapshot foreign keys and add one closed
all-null/all-present shape check. The only valid combinations are
`conversation_turn AND (L0 OR L6)` and `managed_run_execution AND L6`. Half-null
lineage and a source-less ManagedRun reject.

The existing deferred `decodex.enforce_routing_completeness()` trigger function gains
one narrow L0 branch. It can accept only the exact prospective Conversation Turn intent
when the locked Conversation exists, is open, and has the exact expected revision; the
Conversation has no RuntimeSession and no Turn; every candidate member has
`sticky=false`; and candidate membership, policy positions, account revisions, both
quota observations, all eight capability observations, and blocker rows exactly equal
the locked V14 universe. Missing, extra, duplicate, or reordered evidence rejects. Any
source field in L0, sticky member, existing session or Turn, closed or revision-changed
Conversation, ManagedRun consumer, mixed lineage, or evidence mismatch rejects. Its
existing L6 branch stays unchanged, including the requirement for exactly one sticky
member.

The supporting roll-forwards remain in their existing owners:

- `resolve_routing_snapshot_exact` accepts the source RuntimeSession identity/revision
  pair only when both values are present or both are absent. The absent branch proves
  the initial predicates and zero sticky members, then writes exact L0.
- `route_account_exact` applies the same pair rule. It selects in policy order from the
  exact L0 snapshot and replays the exact stored decision. The existing Conversation
  lock makes a conflicting or second cross-key initial decision lose; one decision is
  `Fresh`, and exact-key replay is read-only.
- `plan_initial_thread_continuation_exact` consumes that selected L0 decision and, in
  one transaction, creates the selected account snapshot, copied profile snapshot,
  first revision-1 unfenced `starting` RuntimeSession, and inert `initial_thread` plan.
  Any rejected predicate or write rolls back every one of those rows and their receipt,
  activity, and outbox effects.
- Routing codecs and strict readbacks represent the source pair as jointly optional.
  They permit zero sticky members only for L0 and require the unchanged one-sticky L6
  shape otherwise.
- `decodex.enforce_continuation_plan_completeness()` proves both the selected L0
  decision and the newly created selected-account/profile/session lineage.
- `decodex.enforce_routing_decision_completeness()` is unchanged. V34 does not replace
  it or introduce another routing-decision trigger.

#### Atomic initial admission

The conversations owner admits exactly one Turn with all of these values:

- the prospective Turn UUID bound as intent in the selected routing decision;
- sequence 1 and role `user`;
- `possible_side_effects=unknown`;
- status `active` and revision 1; and
- the exact Conversation and V17-created starting RuntimeSession cross-link.

After V17 creates the session, the Conversation owner uses the prospective UUID and, in
the same transaction, inserts that exact Turn as active revision 1 and exactly one
ordinal-0 completed Message history item. The `Fresh`, `Replay`, or refusal decision,
Turn row, history row, exact receipt, activity, and outbox are one owner transaction.
Exact-key replay is read-only and returns
the stored result. Every competing key, including concurrent cross-key admission, returns
refusal and commits no Turn, history, activity, or outbox effect. A nonzero ordinal,
non-Message kind, second item, wrong Turn identity, wrong role or sequence, wrong side-effect
state, wrong status or revision, or cross-link rejects the whole transaction.

Before ProcessGeneration preparation or spawn, before a thread fence or start, before
thread bind, and before ProviderAttempt preparation or authorization, the effect owner must
lock and require the exact selected Turn to remain active revision 1 under the same
Conversation and V17-created RuntimeSession. ProcessGeneration and thread establishment
through bind require the applicable `starting` revision. ProviderAttempt preparation and
authorization require the exact post-bind `active` revision and exact fence/bind receipts.
A terminalization race loses that fence and cannot start or complete the effect.

Account Service then reads the exact V16-selected account revision, `enabled` state,
AccountLifecycle and exact-build capability, provider binding, credential version and
fingerprint, and actual HostCredentialStore binding. It compares those facts with V14,
V16, V17, and ProcessGeneration intent immediately before spawn. Drift is a definite
pre-spawn refusal. It cannot cause re-selection.

#### Process and effect replay

The exact ProcessGeneration create envelope has four typed outcomes for this path:

- `Fresh` alone returns the non-clone spawn authority and may continue;
- `Replayed` returns durable readback and no spawn authority;
- `Rejected` returns durable refusal/readback and no spawn authority; and
- uncertain or locally lost state returns `Unknown` after bounded durable readback.

`Replayed`, `Rejected`, and `Unknown` cannot spawn, replace, adopt, create a successor,
prepare a duplicate ProviderAttempt, or terminalize the Turn. Recovery uses the existing
ProcessGeneration, RuntimeSession, ProviderAttempt, and Conversation reads. It adds no
ledger or recovery framework; no Turn row exists before the Conversation admission
transaction. The same rule applies after result loss at ProcessGeneration fence or ready,
RuntimeSession thread fence, start or bind, and
ProviderAttempt prepared or authorized. Ambiguous work remains bound to the original Turn
and returns `Unknown` for manual recovery.

The conversations owner may transition the exact active revision-1 user Turn to `failed`
revision 2 while its RuntimeSession is still starting only when positive readback proves a
definite pre-effect refusal. The proof must exclude every ProcessGeneration state that
can have created a child. A fresh result remains definite only after
existing positive spawn-noncreation evidence proves that it created no child. Ambiguous,
replayed, rejected, or uncertain ProcessGeneration state is never definite. The proof
must also exclude a thread fence, thread start or bind, and a prepared, authorized, or
unknown ProviderAttempt. Fenced, thread-started, thread-bound, or
attempt-active/unknown work keeps the Turn active. No other owner can terminalize it.

#### Explicit successor

Explicit successor remains PostgreSQL-only, non-dispatch evidence. It has no protocol
field, product command, runtime `EXECUTE` grant, facade, re-export, fallback, or wake path.
Before it changes a RuntimeSession, creates a Context Pack or snapshot, writes a plan,
activity, outbox, or receipt, its transaction locks the exact Turn named by the routing
decision. The row must belong to the same Conversation and source RuntimeSession, have
status `failed`, and have revision 2.

An active revision-1 Turn, completed revision-2 Turn, absent Turn, wrong Turn, cross-linked
Turn, changed revision, or a terminalization race rejects before every successor effect.
This evidence does not make explicit successor a supported product operation. XY-1304 must
separately reopen and reconcile the Turn and ProviderAttempt lifecycle before any future
caller can be proposed.

#### V34 trigger-function roll-forwards

V34 may replace exactly the seven trigger-bound bodies below. This closed list applies
only to trigger-bound function replacement; it does not prohibit the narrow supporting
roll-forwards of `resolve_routing_snapshot_exact`, `route_account_exact`,
`plan_initial_thread_continuation_exact`, routing codecs/readbacks, the existing
`decodex.authorize_provider_attempt_dispatch_exact(uuid,bigint,uuid,bigint)` command,
or the existing `decodex.complete_exact_continuation_rejection(text,text,text)` helper.
Those functions stay with their current owners and acquire no selection or dispatch
authority beyond the predicates in this section.

| Trigger function | Existing binding, unchanged | Exact authorized Candidate-5 predicate |
| --- | --- | --- |
| `decodex.enforce_routing_completeness()` | `routing_policy_revision_complete`, `routing_evidence_complete`, and `routing_snapshot_complete`, deferred after insert | Preserve the complete existing policy, evidence, and L6 snapshot branches unchanged, including exactly one sticky L6 member. Add only the exact L0 snapshot branch defined above: all six lineage fields null; exact prospective Conversation Turn intent; open exact-revision Conversation; no RuntimeSession or Turn; zero sticky members; and member identity/order, account revisions, two quota rows per member, eight ordered capability rows per member, and blocker rows exactly equal to the locked V14 universe. Reject source fields in L0, partial lineage, source-less ManagedRun, closed or changed Conversation, existing session/Turn, sticky L0, and every missing, extra, duplicate, or reordered child. |
| `decodex.enforce_runtime_session_state()` | `runtime_sessions_state_guard`, before insert or update | Preserve current revision-one insert, identity, timestamp, terminal-immutability, `starting` or `active` to `ended` or `diverged`, and ended-session active-Turn rules. Replace the generic `starting` to `active` edge and add only two other nonterminal edges. An unfenced `starting` row with null thread, last-turn, request, and response fields can advance one revision to `starting` by setting only the complete request ID/digest pair. That exact request-fenced row is the only row that can advance one revision to `active`: it preserves the request pair, sets the response ID equal to the request ID, sets the exact response digest and thread ID, and keeps last-known Turn null. An `active` row can advance one revision to `active` only for an exact last-known-Turn acknowledgement while all thread receipt and binding fields stay unchanged. A generic `starting` to `active`, missing or mismatched receipt halves, response without request, thread binding without response, combined edge, or unrelated field drift rejects. |
| `decodex.enforce_turn_state()` | `turns_state_guard`, before insert or update | Preserve current active-session behavior. Under `starting`, insert only the prospective Turn UUID bound as intent in the selected routing decision, under the exact V17 session and Conversation as sequence 1, role `user`, `possible_side_effects=unknown`, status `active`, and revision 1. Update only that same row from active revision 1 to failed revision 2 while the session is still `starting` and the owner transaction has positive definite pre-effect refusal proof. A completed transition, another role, sequence, status, revision, side-effect value, identity, cross-link, or starting-session write rejects. |
| `decodex.enforce_history_item_state()` | `history_items_state_guard`, before insert or update | Preserve current active-session behavior. Under `starting`, insert only in the admission transaction and only when no item already exists for the exact initial Turn: ordinal 0, kind Message, status `completed`, and revision 1 under the same Conversation. An update, streaming or failed item, second item, another ordinal or kind, wrong Turn, or cross-link rejects. |
| `decodex.enforce_provider_attempt_transition()` | `provider_attempt_transition_guard`, before insert or update | Add only `runtime_session_binding_protocol` and `runtime_session_binding_idempotency_key` to the immutable ProviderAttempt tuple after insert. Keep the current revision-one `prepared` initial state, transition algebra, unknown reason rules, positive terminal-evidence requirement, and every other immutable field unchanged. |
| `decodex.enforce_provider_attempt_binding()` | `provider_attempt_binding_complete`, deferred after insert | Add one `initial_thread` branch. It requires the exact selected V16 decision and V17 plan, selected-account snapshot, ready ProcessGeneration revision and live epoch, exact completed V34 fence and bind receipts, the exact post-bind active RuntimeSession revision, and an existing selected Turn under that Conversation/session with status `active` and revision 1. The two new binding-receipt fields must identify that exact bind receipt. Every non-initial plan keeps both fields null and retains the existing same-thread, Context-Pack, ManagedRun, predecessor, and positive-lineage predicates. An absent, terminal, changed-revision, wrong, or cross-linked Turn rejects. |
| `decodex.enforce_continuation_plan_completeness()` | `continuation_plan_complete`, deferred after insert | Add one Conversation `initial_thread` branch for a selected exact-L0 V16 decision. Require its prospective Turn intent and the new selected account snapshot, copied profile snapshot, one revision-1 unfenced `starting` RuntimeSession, and inert initial plan created by the V17 transaction. Reject non-L0 or partial source lineage, sticky L0, fallback, Context Pack, successor, external effect, alternate account, or extra first session. Keep existing same-thread and Context-Pack predicates unchanged. Explicit-successor completeness must also require the exact selected Turn as failed revision 2 under the same Conversation/source RuntimeSession, but this deferred trigger cannot replace the explicit-successor command's lock before its first write. |

The non-trigger dispatch-authorization command must itself lock and require the exact
initial Turn as active revision 1 under the same Conversation and exact post-bind active
RuntimeSession before it can authorize the provider effect. The non-trigger continuation
rejection helper may only derive the operation from the exact already-reserved receipt so
each existing V17 entrypoint retains its own stable rejection; it adds no transport or
idempotency mechanism. The trigger-only closed list does not authorize any unrelated
non-trigger replacement.

V34 does not replace any other trigger function. In particular,
`decodex.enforce_routing_decision_completeness()` remains unchanged. V34 does not drop,
create, rebind, enable, disable, or rename a trigger. Trigger ACLs and the runtime/PUBLIC
prohibition remain unchanged. Existing active-only behavior remains for every
non-enumerated operation and every unrelated write. The two narrow starting-session
permissions are not a generic starting-session bypass.

V32 remains byte-for-byte the alpha.9.2 four-function capability replacement. V33 remains
enum-only. V34 is the sole unlanded integration migration. There is no V35 or
compatibility DDL. Candidate 5 adds no new module, fixed hierarchy, product or test seam,
ledger, generic transaction or recovery framework, transport/idempotency mechanism,
wrapper, runner, scheduler, or explicit-successor product surface.

Long-term context consists of immutable Project, Advisor, and Program revisions. Project
context records decisions, constraints, repository facts, active Programs/Objectives,
unresolved risks, and accepted handoffs. Advisor briefs compact cross-project status and
risk. Program context records metrics/signals, recent decisions, quiet periods, and next
review. A Context Pack contains the current revision, recent raw window, relevant
artifacts, and repository instructions/OpenWiki. Summaries never silently replace
sources; users can inspect pinned memory and provenance. V1 uses structured PostgreSQL
queries and full-text search, not vectors.

AgentMessage carries logical endpoints, project, correlation and causal parent, dedupe
key, artifact refs, requested response, hop count, and response budget. Deterministic
dedupe, budgets, quiet periods, and causal chains prevent loops. Automation results cannot
recursively wake themselves without a new material signal.
Agents communicate directly only when capability and Project policy permit. Stable
cross-run communication is delivered by Decodex as turns to recipient Conversations.

## Account continuity and profiles

The [account lifecycle authority](account-lifecycle-authority.md) assigns
credential-negative state to PostgreSQL, secret bundles to one HostCredentialStore, and
all account operations to the `decodexd` Account Service. Each app-server process is
bound to one Account UUID. Shared `~/.codex` supplies configuration, plugins, rollout
files, and Codex thread visibility. A refresh callback can supply a newer access token
for the same account, but the account and provider identity never switch under a live
runner. Account
observed state is `unavailable`, `available`, `depleted`, `unknown`, `auth_failed`, or
`plugin_unready`. Administrative `enabled` is a separate versioned boolean; no observed
state sets or clears it. Each quota window stores its class/duration, remaining amount,
reset time, observation time, and confidence; 5-hour and 7-day windows are never inferred
from positional primary/secondary ordering.

`plugin_unready` is inert reserved state; no first-release passive probe sets it.
Codex 0.144.2 and 0.144.4 expose no stable passive complete account-owned plugin,
skill, and MCP readiness receipt, so account-owned readiness remains typed `unknown`.
Host files, desired manifests, configuration, remote catalogs, process/account binding,
and user declarations may be integrity or provenance inputs, but they do not become
observed readiness and cannot prove either readiness or unreadiness.

### Exact quota persistence boundary

Quota storage APIs accept `QuotaTimestampMicros(i64)`, never raw RFC3339 text or arbitrary
nanoseconds. The product-valid interval is `0..=253402300799999999` Unix microseconds. Raw
RFC3339 ingress normalizes offsets to UTC and must be exactly microsecond-aligned before it
constructs that type. Sub-microsecond, pre-Unix, post-year-9999, infinity, overflow or carry,
leap-second, parser-unsupported, and otherwise unsupported values fail before command-receipt
reservation. No application, adapter, database, or migration path rounds or truncates a quota
timestamp. Freshness uses checked integer-microsecond subtraction: an age of exactly 300 seconds
is accepted, while 300 seconds plus one microsecond is stale.

The store owns two canonical mutation schemas: `decodex/quota-window-mutation/2` and
`decodex/quota-exclusion-mutation/2`. Rust constructs them from typed logical values with integer
timestamps, recursively sorted object keys, preserved array order and scalar distinctions, and one
canonical serialization. The receipt binds the resulting SHA-256 digest and byte length, and exact
completed-response replay returns the stored response bytes. Retaining the complete request
document is not required.

V8 is one atomic zero-state migration. In canonical writer order it takes `ACCESS EXCLUSIVE` locks
on `command_receipts`, `quota_windows`, `activity`, and `outbox`, then uses closed structural
classification to reject every pre-V8 quota fact: any `quota_windows` row; every receipt whose
operation is `mutate_quota_window` or whose scope is `quota_windows`, regardless of lifecycle
state; activity classified by aggregate kind, event kind, or structured payload; outbox classified
by aggregate fields or structured activity envelope; every outbox link to classified activity; and
every malformed or orphaned combination of those facts. Correlation-key or aggregate-ID string
patterns are not evidence classification. The assertion and all DDL occur in the same transaction.
Only after zero state is proven may V8 alter `quota_windows` in place, preserving its table identity,
ACLs, account foreign key, unchanged observation-index identity, and migration atomicity while
replacing only changed constraints and adding the typed enums, exclusion relation, indexes, and
authority inventory.

There is no populated V7 conversion or quarantine, hand deletion, retention bypass, table
drop/recreation, dual schema, compatibility read/write, or hidden fallback. Any classified state
aborts V8 with a stable incompatibility result. The supported recovery is to stop Decodex and
recreate the whole disposable pre-release database. XY-1302 owns the final whole-ledger
squash/reset, production baseline, privileges, recreation runbook, cutover/rollback readback, and
proof that no pre-release database becomes production state.

Separate typed 300-minute and 10080-minute observations remain mandatory. This persistence
boundary enables no account assignment, fallback, `waiting_usage` registration, wake scheduling,
continuation, replay of external effects, or live dispatch. Ingress retains the exact raw provider
timestamp value. Construction of UTC Unix microseconds must be exact; any conversion that would
round or truncate is rejected. V14 through V16 consume only exact values, remain otherwise
precision-agnostic, and fail closed. XY-1357 owns the one natural precision receipt in the unified
post-freeze gate. An incompatible receipt leaves production routing disabled and reopens only the
ingress authority; it does not require a moving-core schema or decision rewrite.

The dormant manual account observation path checks an exact PostgreSQL account revision in the
`available` state before mechanics and checks the same predicate again after cleanup. Each check
releases its row and pooled client before arbitrary caller, vault, or process work. The result is a
post-cleanup non-live observation: readiness is not claimed to remain true while the child runs, and
any final stale, non-ready, or unavailable observation fails closed. A blocked synchronous vault can
retain local mechanical capacity, but never a PostgreSQL transaction, row lock, or client checkout.
Runtime owns the sole sibling-adapter composition and all child/vault/protocol mechanics in private,
non-reexported modules. Codex remains a pure capability/schema adapter and does not depend on
PostgreSQL. The private concrete permit moves through every
sequential preflight and the final child, returning to the attempt only after confirmed group death
and child reaping. Uncertain cleanup installs it in a hard-capped fair quarantine and ends the launch
attempt without freeing capacity or synchronously entering an unbounded loop. A stuck group cannot
starve later cleanup. Capacity exists only after its persistent cleanup owner starts successfully;
per-iteration unwind and in-flight-job guards retain ownership across panic, poison recovery, and
wakeup. Cleanup progress never depends on a later launch or external maintenance call. The janitor
belongs to the live capacity lifecycle rather than process-global static ownership: a weak registry
preserves one live daemon authority, and a finite coordinator stops and joins the worker after the
last capacity, permit, and cleanup job is gone. Each permit reserves
one fixed cleanup slot before spawn, so admission has no shared-lock deadline and a 65th group is
rejected before it exists. Cargo metadata proves the current
workspace production dependency graph and absence of synthetic features on normal edges; compile-fail
contracts prove the launcher and capacity authority are not crate API. Neither proves call provenance,
the absence of future wrappers, or Rust friend visibility against arbitrary new downstream
dependencies. This dormant manual path is not product ProcessGeneration authority and cannot grant
restart launch permission.

### Durable ProcessGeneration authority

XY-1400 implements the accepted XY-1398 V3 contract in
[the ProcessGeneration authority specification](process-generation-authority.md).
`ProcessSupervisor` is the sole product writer. A private opaque launch authority retains one
protected executable snapshot and derives the durable launch-manifest identity and exact command.
The manifest binds the image and BuildId, fixed `app-server --stdio` arguments, working directory,
sanitized environment, account, initial account revision, canonical credential version and
fingerprint, provider identity, and exact-build startup/lifetime/account-callback capability. No
caller can pair an independent digest with a raw command. The supervisor commits this intent in
V23 before a fresh fence can authorize one spawn. Intent, launch manifest, prepare fence, ready
transition, and readback carry the same non-secret binding. No new process or effect ledger is
added. The supervisor then binds the exact PID, process-start identity, process group, and session.

The current lifetime profile accepts only the recorded macOS `codex-cli 0.146.0-alpha.9.2` image. It
sets `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` and supplies no remote-control
argument. The marker proves only the exact build's startup state. `ProcessSupervisor` retains the
raw channels privately for lifetime ownership, and no returned ProcessGeneration capability
contains a protocol writer. Other builds, including an unrecorded Linux image, fail closed before
profile-dependent preflights. Generic session/descriptor setup does not install
`PR_SET_PDEATHSIG`; a future Linux parent-death primitive requires a separately accepted exact
Linux lifetime capability. `decodexd` remains the only product daemon.

This profile proves AccountLifecycle readiness only after a positive exact-build callback receipt
and the typed daemon gateway pass the generated-schema and callback-shape preflights. Generated
types, version text, or upstream implementation presence alone are insufficient.
Unsupported builds and callback shapes fail closed before account launch.

The durable states are `starting`, `ready`, `stopping`, `dead`, and `death_unknown`.
All present restored nonterminal rows become `death_unknown`. A generation becomes `dead` only
from positive generation-bound evidence: positive spawn non-creation, owned-child wait, exact
Linux pidfd exit with group quiescence, exact macOS kqueue `NOTE_EXIT` with group quiescence,
exact owned termination exit, or proof that the prior boot ended. PID or process-group absence,
reuse, timeout, lease expiry, row absence, EOF, restart, identity mismatch, and negative search
are never death evidence.

Same-boot uncertainty blocks replacement only for the bound account. Reconciliation continues for
other generations. On macOS, EOF is only a best-effort shutdown request. A restored process can
receive a read-only exact kqueue witness after an exact pre-match and before the final exact
recheck, but it is never adopted, reacquired, proxied, terminated, or signaled. If it exits before
witness attachment, same-boot quarantine remains until boot change. Exact termination is
available only while the original supervisor retains the unreaped child, and it never signals
after reap.

An external execution epoch and digest prevent PostgreSQL restore readback from becoming spawn
authority. Replay never returns a fresh fence. An ambiguous rollback does not authorize launch.
Persisted Codex thread identity carries Conversation continuity; process survival does not create
or continue a RuntimeSession.

ProcessGeneration proves replacement safety only. It does not prove provider non-submission,
effect cancellation, or credential revocation. V24 keeps an unproved authorized ProviderAttempt
`unknown`; process exit or boot change cannot make it `not_submitted`, a replacement cannot replay
it, and any successor is a distinct user-authorized effect with duplicate-risk acknowledgement.
The generic attempt transaction consumes one accepted V16 decision, V17 RuntimeSession, and ready
ProcessGeneration without creating or changing them. XY-1400 adds no account selection, routing,
ProviderAttempt storage, remote authentication, UI, packaging, release, or live dispatch.
A future live-dispatch protocol gateway must be a separate typed authority that source-rejects
alternate-control RPCs before enablement. XY-1400 does not add that gateway.

### Durable routing-policy and candidate-set authority

PostgreSQL owns a revisioned complete routing-authority snapshot and is the only authority that
may construct the candidate universe. A routing-resolution request supplies identity and
idempotency inputs only. Runtime, protocol clients, adapters, and tests cannot supply or override
authoritative policy order, candidate membership, sticky identity, eligibility facts, exclusions,
or a preclassified candidate array.

The current `decodex-core::PolicySnapshot` remains a bounded inert value inside one accepted
Project Policy revision. It neither enumerates the account inventory nor establishes routing
completeness, eligibility, evidence provenance, or persistence freshness. V14 must introduce the
distinct database-owned complete routing snapshot rather than widening a Rust wrapper into
authorization authority.

One snapshot binds, under the database locks that establish its revision boundary:

- the exact accepted routing Policy revision, versioned `fixed` or `balanced` mode,
  optional fixed Account UUID, and canonical user-owned account order;
- every current account-inventory member exactly once, with an explicit included or excluded
  disposition and an exact account revision;
- sticky affinity, when present, plus the exact source RuntimeSession identity and revision;
- required account, RoleProfile, and Codex-build compatibility facts and their exact revisions;
- each exact quota, authentication, capability, and administrative evidence revision used;
- the accepted required-capability set and capability applicability for every member; and
- explicit blocker facts for every unknown or otherwise unusable member.

Completeness is fail-closed. A duplicate, omitted, foreign, newly added, concurrently removed, or
revision-changed inventory member; an unbound sticky source; or an unknown required fact blocks the
snapshot or decision. Silence never means excluded, eligible, or non-applicable.

Selection and pure quota or reconciliation waits classify only included members after independent
eligibility. Excluded members remain ineligible and do not alter those wait classifications.
`no_route` instead projects the complete policy-member universe: every excluded member retains
`excluded_by_policy` and its other persisted blockers, and every included member retains its exact
blockers. An all-excluded universe is an explicit cause-complete `no_route`; a cause-free
`no_route` is invalid.

`decodex-core` is a pure deterministic decision kernel over this database-produced snapshot. It
does not establish provenance or completeness. PostgreSQL atomically persists the resulting V16
decision, its complete normalized exclusions, and every evidence reference. Runtime consumes one
exact persisted decision and sequences effects only; it cannot substitute a decision. Codex is a
positive-evidence capability adapter and cannot determine membership, policy, or eligibility.
One app-server process remains bound to one account, credentials never switch in a live process,
and the separate 300-minute and 10080-minute quota facts for separate accounts are never merged.

Sticky affinity wins only when the bound member is independently eligible under the same complete
snapshot. Eligibility requires the independent versioned `enabled=true` fact; observed health does
not imply enablement. Every known depleted window excludes its account until reset. Unknown, stale,
incompatible, disabled, authentication-failed, missing-duration, low-confidence, or precision-
incompatible evidence blocks eligibility. When every otherwise eligible account is excluded only
by usage, V16 persists `waiting_usage` and the exact earliest-ready time. XY-1362 owns one
restart-safe scheduler wake and always re-enters fresh authoritative resolution; routing owns no
wake lifecycle.

Account-owned readiness is evaluated only for capabilities explicitly required by the accepted
routing Policy revision. Unknown never satisfies a required capability. If the accepted required-
capability set is empty, unknown account-owned plugin inventory is non-applicable; it is not
positive readiness evidence and does not change an account to ready. XY-1336 remains future
passive-receipt tracking. Host-owned before/after receipts prove causal no-mutation integrity only
and cannot establish account-owned readiness.

XY-1358 owns the original causal experiment ledger. XY-1367/V22 repairs its two-effect retained-title
authority without changing V15. For an exact V16 decision with an existing source RuntimeSession,
V17 owns same-thread continuation when exact positive account/profile/build evidence permits it.
Otherwise, that existing-session operation uses one atomic Context Pack plus fallback
RuntimeSession. Candidate-5 initial routing has no source RuntimeSession and instead uses the
first-session `initial_thread` path above; it cannot enter either existing-session branch. V25/V26
preserve the existing-session authority for ordinary Conversation Turns and ManagedRun executions.
The stateless ExecutionCoordinator sequences V16, V17, one live ProcessGeneration fence, and
ProviderAttempt preparation. Current production dispatch stays structurally disabled until its
applicable slice gate passes. Ambiguous-turn replay remains blocked by ProviderAttempt. ManagedRun
consumes the attempt result and keeps only domain lifecycle authority.
Repository/worktree/Git and artifact effects retain their own accepted authorities; routing never
owns or weakens those boundaries.

Those paragraphs define retained final routing authority, not current implementation.
Slice 1 enables only initial selection after the Slice-1 subset of MacDogfoodReady
passes: `fixed` considers its exact target, and `balanced` selects the first fully eligible
account in canonical order.
Selection evaluates both quota windows and returns a typed no-route or all-depleted
result. Recovery is an explicit versioned enable/disable, mode, or order command followed
by a new task. It does not rebind or replay a thread.

[XY-1304](https://linear.app/hack-ink/issue/XY-1304) is the later acceptance owner for
automatic cross-account same-thread fallback and all-depleted scheduler wake. Until its
separate reviewed enablement amendment, those paths and automatic Context-Pack fallback
remain hard disabled. It does not block Quick Task, Project/Lead, ManagedRun, GPUI, or
first Mac dogfood. Replay after an ambiguous outcome remains prohibited independent of
XY-1304 and is reconciled by ProviderAttempt.

Unknown, missing-duration, stale, low-confidence, auth-failed, capability-unready, or
disabled facts never imply eligibility. An all-depleted Slice-1 result exposes reset
evidence and waits for explicit retry; it does not schedule a wake.

Readiness outside the accepted capability-applicability rule cannot authorize account eligibility,
assignment, reassignment, fallback, scheduling, wakeup, continuation, or production routing. A
future operator-triggered active diagnostic requires a separate authority decision and remains
non-routing evidence.

Users exclusively select the four global RoleProfiles. Runtime cannot alter model,
reasoning, or service tier. Each RuntimeSession snapshots its profile. Decodex keeps a
user-owned desired inventory only as intent. Until a stable passive account-owned receipt
exists, V1 reports plugin readiness as `unknown`; that unknown is blocking only when the exact
accepted routing policy requires the corresponding capability, and is non-applicable when the
required-capability set is empty. Host facts never become positive account-readiness evidence or
guide mutation from such a conclusion.

## Automation and protocol

An Automation deterministically turns a schedule, webhook, metric, or repository event
into a deduplicated/materiality- and budget-checked delivery to a Program, WorkItem,
Advisor, or Lead inbox. Lead decision may create a WorkItem/ManagedRun. PubFi, Radar, and
Publisher workflows become Programs plus triggers only when explicitly adopted.

One authenticated, versioned WebSocket multiplexes `control/ack/result`,
`conversation/stream`, `project/work`, `run/activity`, `agent/message`,
`automation/firing`, `accounts/health`, and `system/health`. Commands carry client command
ID, idempotency key, and optional expected revision. Events carry protocol major/minor,
server ID, resumable monotonic sequence, entity ID/revision, correlation/causation, and
payload type. Reconnect is snapshot plus cursor-resumed deltas with backpressure. The
current local product accepts exact protocol V2.0 only; other major or minor revisions
receive a typed refusal before application payload handling.
Large artifacts use authenticated HTTP, never WebSocket snapshots. Non-loopback binding
remains disabled until authentication, TLS, authorization, and redaction gates pass.

GPUI is the primary workspace. Current source opens a real shell and window with a
bounded live Health destination. Every other destination remains a placeholder. The
Quick Task and WorkItem contracts do not make their shell destinations live, and the
app is not generally usable. Slice 1 delivers minimal Accounts, Conversation, and
Health destinations with Quick Task. Slice 2 delivers
Project, Work, and Run destinations for the bounded managed-work flow. Graph/timeline,
automation, broad history presentation, and polish are later obligations. SwiftUI stays
a thin accounts/run-health menubar client over the restricted protocol. GPUI caches are
bounded, disposable, cursor-paginated, and keyed by server/schema/content hash; project
opening never eagerly loads all history.

### GPUI history-page cache authority

PostgreSQL remains product authority. The private `HistoryPageCache` is only GPUI-local,
disposable presentation acceleration subordinate to `HistoryPager`. This first slice is
dormant presentation infrastructure because no current shell destination renders
`HistoryPager`. The pager keeps one active view and its existing four-page, 32-item,
1 MiB in-memory window. This slice does not prove user-visible warm history.

`ClientLifecycle` owns one explicit GPUI cache parent. It starts with the lexical
`std::env::temp_dir()`. On macOS only, when that path starts with the `/var` component,
the lifecycle verifies that `/var` is a root-owned symbolic link whose exact relative
target is `private/var`, and that `/private/var` is a root-owned directory without group
or other write permission. It then replaces only that leading component with
`/private/var`. It does not canonicalize or follow any remaining component. A malformed
or drifted fixed mapping fails lifecycle construction. A non-`/var` macOS path and every
non-macOS path remain lexical. Only after this platform-prefix decision does the lifecycle
append `box.acg.decodex`; tests may inject one exact parent through the private lifecycle
constructor. This is not a generic path resolver, and arbitrary aliases remain prohibited
by the existing no-follow cache boundary. The current unlanded disposable path has no
migration or compatibility obligation. Dormancy causes no history-cache filesystem I/O.
The existing `ClientCache` remains exactly
`<parent>/client-cache`, and `HistoryPageCache` is exactly
`<parent>/history-page-cache-v1`. These sibling caches have separate namespaces,
inventories, locks, failure handling, and local schema generations. `ClientCache` uses
the explicit positive local schema generation `1` for its current schema.
`HistoryPageCache` independently uses local schema generation `1` for its current
schema. The equal values are separate local facts: neither derives from protocol major
or minor, and each cache changes only its own generation when its local schema changes.
`CacheAuthority` keeps the stable server ID, protocol major, protocol minor, and
`ClientCache` local schema generation as separate identity fields. This cache is not the
`decodex-core` typed
`~/.decodex/cache` authority, adds no `decodex-core` dependency, and is not a generic
cache framework. It must never publish into, count against, mutate, dispose, switch,
quarantine, recover, or otherwise change a `ClientCache` generation, checkpoint binding,
inventory, path identity, or lifecycle state.

The closed local schema ID is `decodex.gpui.history-page-cache/1`. One entry has the exact
identity `(stable_server_id, protocol_major, protocol_minor, cache_schema_generation,
conversation_id, request_key, page_sha256)`. `ClientLifecycle` supplies the positive local
`cache_schema_generation`; it is exactly `1` for this schema, independent of protocol
negotiation. Protocol major and minor remain separate identity fields, and this cache
changes no protocol. `request_key` is either `head` or the exact opaque `after` cursor
from a fresh live page; cursor text is not normalized. For schema `/1`, the page bytes and
the bytes hashed for `page_sha256` are exactly
`serde_json::to_vec(&ConversationHistoryPage)` with the protocol type's unchanged serde
definitions and version. `page_sha256` is the lowercase SHA-256 digest of those complete
bytes. No other JSON canonicalization or key sorting applies. Any serialization change
requires a cache-schema bump. The digest is verified on each read. One
authority/Conversation/request tuple points to one digest, and an admitted fresh response
atomically replaces its older index mapping. An unknown schema or a changed protocol
major, protocol minor, local cache schema generation, server, key, page size, bytes, or
digest is incompatible cache data.

Opening a view enqueues its fresh head request. Binding or replacing a transport session
does the same for an active view. The existing retained-session snapshot application and
confirmation gate controls when the transport may send that queued request; cache work
does not bypass the gate. An authority-matched provisional lookup may start only after
the transport successfully sends that exact queued request. A send failure starts no
lookup. On session replacement, `HistoryPager` first cancels old work and clears every
fresh and provisional page and all cursor topology. Only a new successful send may then
start the matching post-send lookup. An admitted fresh page may independently start
publication. No history-cache filesystem I/O occurs before the first exact post-send
lookup starts or an admitted fresh page starts publication. Cache I/O must not delay,
suppress, replace, or deduplicate the queued fresh request.

A hit has source `cached_unverified`, never `FreshServer`, and has cursor observation
`unknown`. It does not prove that content is current or present, or prove absence,
completion, no continuation, or cursor validity. A matching cached head may populate the
provisional `HistorySnapshot` only after its exact fresh head request was successfully
sent and while that request is in flight. A continuation lookup is permitted only after
a fresh page supplies the exact live
`next_cursor` used as its `after` request key. The lookup preserves the live
`RequestPurpose`; a cached prefetch never becomes visible or changes topology. A later
visible request is distinct and must perform its own authority-matched lookup. A cached
`next_cursor` never drives navigation or prefetch. Only fresh server pages create
topology. A matching fresh response replaces or validates provisional bytes and then has
source `FreshServer`.

`fresh_received_at` is the immutable server-admission wall time for a fresh response. An
entry is eligible only when `now >= fresh_received_at` and
`now - fresh_received_at < 15 minutes`; a hit does not extend that interval. There is no
wall-clock high-water mark. Clock rollback cannot grant authority because a future-dated
entry is ineligible, every hit remains provisional, and a fresh request is always queued.
An invalid timestamp, an expired entry, or an authority, key, page-size, byte-length, or
digest mismatch is refused or removed under the cache lock. The server's one-hour cursor
expiry remains unchanged.

All cache limits apply at the same time:

- one page has at most eight items and 256 KiB of the schema-defined page bytes;
- one Conversation has at most four pages, 32 items, and 1 MiB;
- the cache has at most eight Conversations, 32 pages, 256 items, and 8 MiB of
  schema-defined page bytes; and
- the index has at most 64 KiB, and the sum of all cache-owned regular-file lengths,
  including one staged page and one staged index, never exceeds the 9 MiB physical
  staging peak.

Eligible hits update bounded in-memory LRU recency and require no durable write. The next
cache-changing index publication persists the current recency of retained entries. Losing
recent hit order on crash is allowed. After restart, eviction uses the last persisted
recency and the exact entry identity in ascending byte order as the deterministic
tie-break. Eviction first removes invalid or expired entries. It then enforces
per-Conversation limits by oldest recency, removes the Conversation whose newest entry has
the oldest recency while the Conversation cap is exceeded, and enforces aggregate limits
by oldest entry recency. Correctness does not depend on an orderly-shutdown flush.

At lazy open, `HistoryPageCache` requires the configured parent to be absolute. The fixed
macOS platform-prefix decision above is already complete and has constructed the common
parent for both sibling cache paths. `HistoryPageCache`
lexically splits that parent into the existing external base `parent.parent()` and the
unchanged final cache-parent leaf `parent.file_name()`. It requires both values and
requires the external base to exist. It canonicalizes only that external base. The
canonical result must be absolute and normal: it has one root, and every component after
the root is normal. Starting from `/`, it opens each component of the resolved base
descriptor-relative with `O_NOFOLLOW`. Each opened component must be a directory owned by
root or the effective UID. Group or other write permission is prohibited except on a
root-owned sticky directory. Only canonicalization of the complete external base may
resolve a lexical alias for this cache's private descriptor. It grants no authority to an
arbitrary `ClientLifecycle` parent: the eager `ClientCache` no-follow validation rejects
that parent before history-cache use. No component-level rule permits an arbitrary lexical
symlink ancestor or permits a symlink because root owns it.

From the validated resolved-base descriptor, `HistoryPageCache` opens or creates the
unchanged final cache-parent leaf descriptor-relative with `O_NOFOLLOW` and validates it
as a mode-`0700` directory owned by the effective UID. It never canonicalizes or follows
that leaf, `history-page-cache-v1`, or any descendant. The resolved base and its
descriptors remain private to `HistoryPageCache`. Resolution must not rewrite
`ClientLifecycle.cache_parent`, pass a canonicalized path to `ClientCache`, or change
`ClientCache` path identity, binding, generation, inventory, quarantine, or lifecycle
state. The earlier fixed macOS component replacement is complete before this lazy
page-cache-only resolution. Neither operation is a generic path resolver or framework.

The fixed owner-private filesystem shape is
`history-page-cache-v1/{lock,index,.index.next,pages/<page_sha256>,pages/.page.next}`.
Every index read or mutation, hit lookup, publication, eviction, remnant removal, and
cleanup is serialized by the same cache lock. Canonicalization of the existing external
base is the sole explicit exception to the descriptor-relative no-follow rule. The final
cache-parent leaf, `history-page-cache-v1`, and every cache-owned component are opened
descriptor-relative with `O_NOFOLLOW`; none is canonicalized or followed. Directories
have mode `0700`; regular files have mode `0600`, one link, and the effective user as
owner. Name counts, file lengths, owners, modes, link counts, and file kinds are bounded
and validated. A foreign name, link, owner, mode, or file kind disables this cache. The
cache remains inside the existing trusted same-UID boundary. It does not claim confinement
against hostile code that already has the same effective UID. The implementation must use
the existing workspace `libc` dependency for the descriptor-relative primitives.
Hand-written Darwin FFI and any new crate or framework are prohibited.

Before creating a stage file, the lock holder validates the complete known shape, removes
only known stage remnants and valid digest-named pages not referenced by the validated
index, and computes the complete physical peak. If publication must reclaim referenced
space, it first publishes and syncs an eviction-only index, then deletes only pages that
the published index no longer references, syncs `pages`, and recomputes the peak.

Page publication creates and syncs `pages/.page.next`, then publishes the digest page
create-only. If the exact digest target already exists, the cache verifies its exact
bytes and digest and reuses it; it never replaces that target. After publication or reuse,
`pages/.page.next` must be absent and `pages` must be synced before `.index.next` is
staged. Publication then creates and syncs `.index.next`, replaces `index`, and syncs the
cache root before it treats the mapping as durable. Only after the new index is durable
may cleanup delete pages that it no longer references, followed by a final `pages`
directory sync. Any cleanup failure disables only this cache. The index is cache metadata,
not a credential vault, migration, compatibility layer, or product ledger.

Each cache lookup and write carries the exact active view generation, transport session
generation, stable server, protocol major, protocol minor, lifecycle-supplied local cache
schema generation, Conversation, and request generation. It may publish or become visible
only if all values still match at completion. Cancellation or replacement drops staged
work, and stale completion is never shown. Crash, corruption, I/O, bounds, or cleanup
failure may delete this cache only after safe validation; otherwise it disables only this
cache. Parent resolution or validation failure disables only `HistoryPageCache`.
History-cache work cannot delay or suppress the already-required fresh transport send.
Cache open or publication failure cannot alter, reject, or make unusable an
already-admitted fresh page. History-cache failure never starts lifecycle quarantine or
recovery and never changes `ClientCache`.

The first implementation write set is only
`apps/decodex-gpui/src/history_pager.rs`, the new
`apps/decodex-gpui/src/history_pager/page_cache.rs` and its module-local tests,
`apps/decodex-gpui/src/client_lifecycle.rs`, and
`apps/decodex-gpui/src/client_lifecycle/tests.rs`, plus
`apps/decodex-gpui/Cargo.toml` solely to add the existing workspace `libc` dependency.
`client_cache.rs`, `main.rs`, the root `Cargo.toml`, and all protocol, runtime, PostgreSQL,
account, Swift, and FFI paths remain unchanged. Hand-written Darwin FFI, any new crate or
framework, a generic cache framework, a ledger, a protocol change, and a user-visible
destination remain prohibited. The active XY-1427 account lane owns `Cargo.lock`;
regeneration and integration of that lockfile is one deferred mechanical step after
XY-1427 lands and is not performed by the isolated source writer. The implementation is
not ready or landable until that exact lockfile integration and locked validation
complete. Local full-text search is deferred to future authoritative server/PostgreSQL
work. Cached cursors never drive topology, and cache failure never enters lifecycle
quarantine or recovery.

Acceptance is limited to:

- `HistorySnapshot` reports `cached_unverified`, cursor `unknown`, and no
  cached-cursor topology; it preserves `RequestPurpose`, never exposes a cached prefetch,
  and replaces or validates provisional bytes with the matching fresh response;
- cancellation and exact active-view, request, transport-session, stable-server,
  protocol-major, protocol-minor, and local-cache-schema identities prevent stale lookup,
  publication, or visibility;
- bounded platform evidence covers the validated fixed macOS `/var` to `/private/var`
  lifecycle-prefix replacement, refusal of arbitrary aliases, and the lexical Linux
  `/tmp/box.acg.decodex` temporary-path shape while preserving the unchanged final
  cache-parent leaf;
- an instrumented lifecycle test proves that no history-cache filesystem I/O occurs
  before the successful transport send of the exact request or the start of an admitted
  fresh-page publication;
- filesystem tests refuse a symbolic link at the final cache-parent leaf,
  `history-page-cache-v1`, or any descendant;
- deterministic tests cover time eligibility, restart recency, cache limits, eviction,
  corruption, known remnants, publication ordering, and publication and cleanup faults;
  and
- one lifecycle-focused test injects parent-resolution, validation, and page-cache
  failure and proves that the fresh result remains unchanged and usable and that the
  existing `ClientCache` generation, checkpoint binding, inventory, connection quarantine,
  and lexical path identity do not change.

User-visible warm history is not an acceptance claim until the Conversation destination
consumes `HistoryPager`.

## Clean cutover and delivery

Cutover has no availability requirement. Stop v0.2 and start vNext with empty PostgreSQL
execution and control-plane state. Import each local account once through the ordinary
versioned account-import command from an owner-private temporary file. Verify the
PostgreSQL account and routing readback, the exact HostCredentialStore binding, and the
per-account Reset Card readback. Then delete the temporary import files and the retired
local account source.

The product has no account-migration manifest, bulk importer, migration receipt,
migration state machine, migration finalizer, compatibility fallback, or dual account
authority. It imports no quota, usage/profile projection, account history, Codex
sessions, SQLite execution state, Linear lanes, or Codex-created tasks. Normal startup
reads only vNext PostgreSQL and HostCredentialStore authority. Recreate selected Projects
explicitly from reviewed inventory.

Delivery has exactly three dependencies: Accounts/Quick Task/Accounts-Conversation-Health
GPUI, then the bounded Project/Lead/ManagedRun flow and Project-Work-Run GPUI, then the
two-account self-hosting restart E2E and Mac package. The exact gates and the
MacDogfoodReady-versus-final deferred table are in the
[gate manifest](vnext-gates.md#delivery-slices).

Freeze/close PR #1092 and do not cherry-pick its implementation wholesale. Every task
uses a focused worktree branch and PR directly into `main`; there is no long-lived vNext
branch. Product-incomplete `main` is acceptable during replacement only when each merge
compiles, tests, and states current capability. Remove Linear, SQLite product authority,
Goal, and old operator transport after replacement behavior and gates exist; do not add
dual writes, dual reads, or compatibility facades. Radar, Publisher, and the static site
may remain outside the runtime until explicitly adopted.

PR #1092 is closed, unmerged, and frozen at historical head
`32a0589b94987f265013ffd3c8b322f9c57f5097`. Its Lane Authority v2 identity, Linear scope,
SQLite registry, lane/effect ledger, and C1-C7 orchestration are obsolete. The only relevant
behavior classes are already replaced by vNext owners: explicit Project/repository identity by
the Project and managed-repository admission contracts, frozen admitted-base and worktree
continuity by the V13/executor/saga stack, and paginated positive GitHub readback by the sealed
GitHub effect boundary. It contributes no unique production behavior to the vNext candidate.

## V1 non-goals

- Pi as a second runtime; per-run/per-agent `CODEX_HOME`; Codex Project sync.
- Linear import, projection, identity, lane authority, or compatibility.
- SQLite product authority, dual writes, historical Codex/SQLite execution migration.
- Domain Agents, automatic multi-Lead, arbitrary durable roles, or Goal as general
  planning/review/development state.
- Graph/vector databases, event sourcing, CRDT/DeltaDB worktrees, distributed workers.
- Unauthenticated remote control or runtime-selected model/reasoning/service tier.

## Decision-changing evidence

Only the falsifiers in the owning decision may revise this contract. A failing gate
freezes the affected milestone and records the contradiction; it does not authorize a
silent legacy fallback.

# Decodex vNext Authority Contract

Status: normative target contract; implementation is gate-controlled. The XY-1403
private-artifact retirement takes effect only at the exact repository effective point
in the [retirement decision](private-artifact/decision.md#repository-effective-point).

Owner: [vNext authority decision](../decisions/vnext-authority.md). Gates:
[vNext gate manifest](vnext-gates.md).

There are no external or deployed users. The current local PostgreSQL database is
disposable development state. This contract has no supported schema-upgrade path.

Decodex has exactly one canonical unversioned latest schema source:
`crates/decodex-postgres/schema.sql`. Numbered SQL, Refinery, schema-history ledgers,
upgrade prefixes, compatibility DDL, migration receipts/finalizers/fallback, Phase A/B
schema receipts, schema generators, and second executable schema owners are rejected.
The former schema labels, including V14, V16, V17, V33, and V34, are superseded
provenance and are not current owner names.

One explicit operator bootstrap owns clean-target precondition, one transactional schema
execution, and exact post-execution verification. A second bootstrap against a nonempty
target fails closed. Normal `decodexd` startup resolves no schema-owner credential,
executes zero DDL, and verifies only the exact current catalog and configured authority.

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
product entity in the first release. Task and Reviewer agents are execution-scoped. Codex-native
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

The latest schema contains no ManagedRun-local submitted-turn receipt, safety-input,
effect, or barrier authority. Those old shapes have no compatibility writer and no data
conversion path. Current local data is disposable.

Task and Reviewer assignments remain exact-run RuntimeSession identities. Their closed
role type cannot represent Advisor or Lead and contains no durable Agent identity.
ManagedRun owns run lifecycle, wait state, review, acceptance, and completion.
ProviderAttempt is the sole external-turn attempt, receipt, ambiguity, and positive
outcome authority for both ordinary Conversation Turns and ManagedRun executions.
Missing, empty, exhausted, not-found, scan-exhaustion, no-event, or method-result absence
never authorizes progress or proves non-submission.

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
| v0.2 state | Final vNext runtime and installer read none. The reviewed local database reset may preserve the complete credential-negative account/routing/binding tuple, including each enabled state and the mode-valid fixed target, and rebind it to unchanged host-vault records. Only the existing HostCredentialStore owner may perform a confined in-process read for typed credential-negative agreement; no token bytes leave that owner. |

PostgreSQL is not event sourced and no graph database is used. Stable IDs plus correlated
activity derive graph/timeline projections. `decodexd` is the sole product scheduler,
app-server child owner, mutation coordinator, and repository-side-effect owner. GPUI,
SwiftUI menubar, CLI, and MCP are clients/adapters over common application services; they
never read PostgreSQL, rollout files, blobs, or repositories directly. The first release is single-host
and has no worker registry or distributed mesh. Remote UI may be added only through the
protocol security gate.

### PostgreSQL latest-schema authority

`crates/decodex-postgres/schema.sql` is the only executable schema owner. It contains the
final accepted enums, relations, constraints, indexes, functions, triggers,
dependencies, ownership, and ACLs directly. It contains no old-state branch, drain,
backfill, compatibility operation, or reverse operation.

An allowed Rust `schema` module must own clean-target verification, one transaction that
executes the complete schema, and post-execution current-authority verification before
commit. A wrapper around `include_str!` alone is prohibited. There is no schema manager,
registry, version constant, generator pipeline, bootstrap facade, or cutover coordinator.

The explicit operator bootstrap resolves the schema-owner credential for that invocation
only. It requires an empty PostgreSQL 18 target with data checksums, runs the complete
schema once, verifies `pgcrypto`, exact catalog shape, stable dependencies, ownership,
ACLs, function/trigger bodies and settings, `schema_fingerprint`, and configured runtime
authority, and commits only after all checks pass. A second bootstrap fails because the
target is no longer empty.

Normal `decodexd` startup resolves only the runtime credential. It runs zero DDL and
performs the same read-only current-catalog/configured-authority checks. It never creates,
upgrades, repairs, or finalizes a schema and never reads a schema-history relation.

### Runtime composition and readiness

One `decodexd` and one owner-only same-UID endpoint serve all product and diagnostic
surfaces. Quick Task execution or ManagedRepository unavailability does not make daemon
startup fail when the transport and control plane can start. Diagnostics, account
recovery, and each available PostgreSQL-backed read remain reachable.

`ProductStore` has exactly two startup results:

- `Available(PostgresStore)` after exact runtime PostgreSQL and current-authority
  verification; or
- `Unavailable(ProductStateReason)` with no retained store.

`ProductStore` means verified PostgreSQL only. Quick Task, repository, Git, path,
reconciliation, and optional repository-configuration results cannot replace, erase, or
change it.

All fallible Quick Task dependencies must finish their own validation, I/O, attestation,
and startup before composition constructs `QuickTaskRuntime`. Construction is infallible
and performs no I/O. Composition records exactly one immutable process-lifetime
projection:

- `Ready(QuickTaskRuntime)`; or
- `Unavailable(QuickTaskUnavailableReason)`, where the reason is closed and redacted.

This projection is not a mutable capability manager, lifecycle, authority, cache,
receipt, retry owner, or substitute for current evidence. Every execute, start, and resume
command repeats all accepted owner fences before an effect. The projection never becomes
ready without daemon restart.

ManagedRepository has one independent optional startup projection: `Ready`, `Disabled`,
or `Unavailable(ManagedRepositoryUnavailableReason)`. Absence is `Disabled`. Invalid
repository-only configuration and path, Git, executor, or reconciliation failure are
`Unavailable`. They disable repository operations only and cannot block endpoint binding,
PostgreSQL verification, account recovery, or repository-free Quick Task work.

Core runtime configuration contains transport and PostgreSQL runtime inputs. It does not
require `server_host.repositories`. Repository identity, admission, and persisted path
policy remain PostgreSQL authority. A host-only repository configuration is permitted
only when a concrete accepted host policy consumes it. Its parser and validator are
separate from core configuration, so absent or malformed repository data cannot block
core parsing or service assembly.

Immediately before a Quick Task process spawn, the runtime host adapter validates the
selected working directory by no-follow descriptor traversal, exact descriptor identity,
directory type, ownership by the daemon effective UID, and the applicable accepted path
policy. A request path, ambient current directory, or repository discovery is not
authority. One unrelated broken repository cannot disable all Quick Tasks.

Protocol and doctor project ProductStore, Quick Task, and ManagedRepository readiness as
three independent typed fields. Quick Task execute, start, and resume return
`QuickTaskUnavailable(reason)` when the Quick Task projection is unavailable. Persisted
Quick Task list and get retain `ProductStateUnavailable` when PostgreSQL is unavailable.
No `.ok()` conversion, optional setter, omitted field, or generic integrity error may
hide the assembly result. `AcceptanceUnknown` and recovery-required results retain their
existing effect and recovery semantics.

The verifier rejects any extra, missing, changed, unsafe, or unreachable schema,
relation, column, enum, constraint, index, function, trigger, rule, policy, RLS setting,
sequence, dependency, owner, or grant. It closes inherited and `SET ROLE` authority,
PUBLIC access, grant options, DDL, `TRUNCATE`, trigger bypass, unsafe function settings,
hostile `search_path`, overloads, external cascades, and extension-member control.

Exact-command receipts, account operations, ProcessGeneration, ProviderAttempt,
`schema_fingerprint`, runtime authority, activity, outbox, and repository-effect records
remain current domain integrity. They are not schema history or bootstrap permission.

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

Within the trusted single-host first-release boundary:

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
  outside first-release confinement. Hostile-project or multi-tenant operation requires a separate
  UID or sandbox owner and an independently accepted feasibility and authority gate.

ManagedRepository service assembly is optional. Its `Ready`, `Disabled`, or typed
`Unavailable` projection does not change ProductStore or Quick Task readiness. A static
host repository map cannot duplicate PostgreSQL repository identity, admission, or path
policy. Any accepted host-only policy is parsed separately and affects only repository
operations.

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
redefining current authority. The first release has no external monotonic anchor and no automatic
full-cluster rollback detection. The accepted trusted single-daemon/same-UID boundary,
XY-1354 descriptor-assisted symlink-free persisted absolute-path reacquisition, and
pinned Git 2.54 mechanism remain unchanged.

Managed Repository Persistence owns the final physical relations, transaction mechanics,
privileges, retention, and current database evidence. Repository Executor owns read-only
acquisition plus executor/readback mechanics, not persistence, receipt minting, saga, or
hidden allocation mutation. Repository Saga owns the shared path that composes preparation,
fresh receipt consumption, execution, readback, and terminal reconciliation. Rejected candidate trees
`6e20e9b3cf1415cce9b399da173b0410cc4c80dc`,
`6979e3831da772fca3fe0f0e0b4699df642d3a65`, and
`e42212add13af3f702e0ec8966ce3d6a7b682d12` are superseded evidence only.

Pure PostgreSQL commands use a different, exact in-transaction authority. Each operation has one
command-complete schema-owner `SECURITY DEFINER` function. PostgreSQL constructs the complete
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

XY-1345 records the exact-command protocol. RoleProfile Authority owns the separate
receipt relation and RoleProfile bootstrap/update. RuntimeSession Authority owns
authoritative RuntimeSession snapshot creation/transition. Candidate 3 is superseded
code and may supply only independently re-derived invariants and hostile-test ideas.

RoleProfile Authority persists exactly the `advisor`, `lead`, `task`, and `reviewer` identities in
`role_profiles`, keeps every configuration in immutable `role_profile_revisions`, and advances one
current-revision pointer per role. One initial bootstrap seam accepts user-supplied typed server
configuration containing all four role-implied advisor/lead/task/reviewer scalar groups and invokes
`bootstrap_role_profiles_exact` to create all four revision-one profiles atomically. PostgreSQL then
owns every revision and current pointer. Later changes use only `update_role_profile_exact`, which
accepts one typed role plus an expected revision, appends exactly one immutable revision, and
advances only that role's pointer. Both functions return and retain PostgreSQL-built response bytes
whose effects are assembled from the returned profile rows and the actual canonical
activity/outbox identities. Routing and runtime never derive, select, or override model, reasoning,
or service tier. Quick Task requires the current `task` RoleProfile; absence is a typed
`QuickTaskUnavailable` initialization refusal and never authorizes synthesized defaults.

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
Experiment Authority owns intent and the first creation fence. Retained-title Authority binds the exact
nullable-name start response, fences the separate name-set effect, and requires exact-ID
retained-title attestation before positive observations or same-thread authority. Production
Quick Task creation belongs to XY-1276. External Codex activity may be provenance-imported for ordinary
Quick/Advisor/Lead conversations; on an active ManagedRun it marks the session `diverged` and blocks
side effects until tool/repository readback reconciles them.

### Quick Task thread establishment

Status: Candidate 5 is approved target architecture. Implementation and integrated
acceptance remain pending. Candidate-4 tree
`f82b866e21f12742648023a2b468cc057afa52a1` is rejected provenance.
This authority amendment changes no schema or product code, creates no migration, records no
validation evidence, and does not claim live Quick Task success.

Quick Task remains an ordinary multi-turn Conversation. Candidate 5 uses existing owners
in this exact order. Its ordinary initial account selection is independent of Project and
accepted Project routing policy. It requires no current Project routing-policy row, accepted
Project policy, per-account `routing_compatibility_evidence`, or `quota_windows` projection. This
selection occurs only while establishing the first RuntimeSession:

1. Conversation authority creates the Conversation.
2. Routing receives a prospective Turn UUID as intent. It creates no Turn, and no routing
   column has a foreign key to `turns` for that intent.
3. The Quick Task routing adapter starts one transaction, locks the Account Registry's complete
   current authority, and materializes Routing Snapshot without a source RuntimeSession or sticky
   member.
4. The existing Routing Decision selects the account exactly once in that transaction and persists
   the immutable decision.
5. Continuation Plan consumes that selected decision and atomically creates the selected
   account snapshot, copied RoleProfile snapshot, one revision-1 unfenced `starting`
   RuntimeSession, inert `initial_thread` plan, exact receipt, activity, and outbox.
6. Conversation authority admits the exact prospective Turn and first history item in one
   transaction.
7. Every establishment fence locks and rechecks that Turn as active revision 1 under the
   same Conversation and new RuntimeSession.
8. Account Service fences only the selected account immediately before spawn.
9. Only a fresh ProcessGeneration fence may spawn.
10. RuntimeSession Thread Establishment owns thread request fencing, exact start binding,
    activation, and acknowledgement.
11. ProviderAttemptService owns preparation, authorization, ambiguity, positive evidence,
    and reconciliation.

There is exactly one selecting Routing Decision for an ordinary Quick Task. Initial planning
creates the first session; it is not explicit successor, same-thread reuse, or Context Pack
fallback. Every later Turn uses a non-selecting immutable continuation route binding to the
current RuntimeSession and its original initial decision, selected account snapshot, and copied
Task RoleProfile snapshot. Same-thread and Context Pack planning retain that exact account and
profile. Later routing never calls `read_current_task_routing_authority_exact()`, resolves another
Account Registry snapshot, or runs selection. Selected-account drift, exhaustion, or readiness
failure returns typed manual recovery without fallback, wake, alternate account, or re-selection.

| Owner | Candidate-5 authority |
| --- | --- |
| Account Registry | Complete non-tombstoned membership, canonical routing control and revision, account revisions, eligibility blockers, and current independent account-quota facts; no selection. |
| Quick Task routing adapter | One transaction that materializes the only selecting Quick Task Routing Snapshot and Decision from locked Account Registry facts; later Turns receive non-selecting continuation bindings. |
| Project routing | Accepted Project policy and compatibility evidence for ManagedRun and other policy-bound `L6` work; no ordinary initial Quick Task authority. |
| Account Service | Account operations and exact selected-account readiness/credential/provider/HostCredentialStore pre-spawn fence; no selection. |
| Routing Decision | Sole Quick Task account selector for first-session establishment and immutable route/continuation-binding writer. |
| Continuation Plan | Initial snapshots, first starting RuntimeSession, inert initial plan, and same-thread/Context Pack planning that retains the original account/profile binding. |
| Conversation authority | Conversation creation, atomic initial Turn/history admission, legal Turn finalization, and exact Turn lock/read proof. |
| RuntimeSession Thread Establishment | RuntimeSession state/thread fields, exact thread fence and bind, and acknowledgement. |
| ProcessSupervisor | ProcessGeneration intent, fresh spawn authority, exact readback, positive death evidence, and account-local quarantine. |
| ProviderAttemptService | Attempt preparation, dispatch authorization, ambiguity, positive evidence, and reconciliation. |
| ExecutionCoordinator | Crate-private stateless sequencing only. |
| QuickTaskRuntime and ServiceApplication | Sequence and consume typed owner results only; no account, policy, profile, or capability selection. |

Account Service can report and fence facts for the selected account. It cannot preselect,
substitute, fall back to, or wake another account. Fixed mode accepts only its exact fixed
eligible member. Balanced mode selects the first eligible member in canonical Account Registry
order. Continuation Plan creates only the selected member's snapshots and RuntimeSession. Later
selected-account drift fails closed before spawn without a second decision or another account.

#### Closed routing lineage

For `routing_snapshots`:

- `L0` means all six source lineage fields are null: `runtime_session_id`,
  `runtime_session_revision`, `account_snapshot_id`,
  `account_snapshot_source_revision`, `profile_snapshot_id`, and
  `profile_snapshot_source_revision`.
- `L6` means all six fields are present and the three revisions are positive.

Source lineage retains its closed all-null/all-present rule. Conversation initial selection uses
`L0`; a later Conversation continuation binding uses `L6`; ManagedRun uses `L6`. Half-null lineage
and a source-less ManagedRun reject.

Selecting snapshots in the existing routing relations have exactly two `authority_shape` values:

- `conversation_account_registry` is permitted only for initial Conversation `L0`. It binds the
  exact Account Registry routing revision/mode/fixed target/order, current Task RoleProfile
  revision, complete non-tombstoned membership, exact account revisions,
  enabled/lifecycle/health/credential-binding blockers, and exactly two quota slots per member.
  Project policy/evidence/build fields are null and no Project capability or compatibility rows
  are consumed.
- `managed_run_project_policy` is permitted only for ManagedRun `L6`. It retains the existing
  accepted Project policy, compatibility evidence, capability, and Project-era quota shape.
  Account-Registry-specific routing/profile/quota-slot fields are null.

Reverse-shape constraints reject mixed fields, consumers, candidate children, or evidence. Each
Account Registry quota slot has `duration_minutes` equal to `300` or `10080` and preserves one exact
`account_quota_facts` source state:

- `missing`: `used_percent`, `observed_at`, `resets_at`, and `error_code` are all null;
- `current`: `used_percent`, `observed_at`, and `resets_at` are present and `error_code` is null; or
- `observation_error`: typed `error_code` and `observed_at` are present while `used_percent` and
  `resets_at` are null.

The snapshot fabricates no quota revision, remaining value, confidence, evidence provenance, or
legacy `quota_windows` value. The initial decision references that immutable snapshot and persists
only `selected`, `waiting`, or `no_route`, plus exact exclusion reasons needed for replay. Fixed
selection accepts only the exact fixed eligible member. Balanced selection follows canonical
Account Registry order and classifies the two slots independently without a merged quota pool.

The existing decision relation also has a closed `conversation_continuation` decision shape. It is
non-selecting, has `L6` source lineage, directly references the current RuntimeSession and original
initial selected decision, and repeats only that decision's account snapshot and copied Task
RoleProfile snapshot identities. Candidate, policy, quota, exclusion, waiting, and selection fields
are null. Reverse constraints require exact identity/revision equality with the source lineage.

The supporting current owners obey these predicates:

- The Quick Task routing adapter locks the canonical Account Registry routing row, complete
  non-tombstoned membership, account revisions, blockers, current Task RoleProfile, and both quota
  slots, then invokes snapshot resolution and route decision in one transaction. It does not call
  `read_current_task_routing_authority_exact()` or create a Project-policy compatibility bridge.
- `resolve_routing_snapshot_exact` accepts source RuntimeSession identity/revision only when both
  are absent for Quick Task. It is used only for initial selection, consumes the locked Account
  Registry facts, proves zero sticky members, and writes `conversation_account_registry` `L0`.
- `route_account_exact` uses canonical Account Registry fixed or balanced control only for initial
  selection and replays only its stored decision.
- `bind_quick_task_continuation_exact` creates the immutable non-selecting continued decision from
  the locked Conversation/source RuntimeSession and original initial decision. It never reads
  current Project routing, resolves a snapshot, invokes the decision kernel, or changes
  account/profile identity. Same-key replay returns the stored binding; a competing key for the
  same prospective Turn creates no second binding.
- `plan_initial_thread_continuation_exact` consumes only selected `L0` and creates the
  complete first-session authority cluster in one transaction.
- Same-thread and Context Pack planning consume only `conversation_continuation`, retain the exact
  original account and Task RoleProfile snapshots, and return typed manual recovery for selected
  account drift, exhaustion, or readiness failure.
- Routing codecs/readbacks preserve the closed snapshot and decision discriminators. Decision
  completeness enforces selecting classification/exclusions or continued lineage, never both.

#### Initial selection concurrency

Initial selection is one top-level `READ COMMITTED` exact command. A completed same-key replay
returns its stored response without domain work. A fresh execution takes authority locks in this
order: the Conversation intent first; the
`account_routing_control` singleton; every complete non-tombstoned account row in canonical UUID
order; and the current Task RoleProfile row. It then reads and copies `account_routing_order` and
both duration slots from `account_quota_facts` while retaining those locks through commit.

Every enrollment, tombstone, order, fixed-target, or mode operation that can change membership or
routing locks `account_routing_control` before the affected or complete account set in the same
UUID order. Every account-local quota observation locks its account row before it inserts or
updates the quota row and never takes `account_routing_control` afterward. Therefore selection's
account locks also serialize an insert for an absent quota slot, and no routing/quota lock cycle is
possible.

Before commit, the command compares the locked routing revision/control/order, Task RoleProfile
revision, account membership/revisions/blockers, and both exact quota tri-states with the copied
snapshot. Any stale expected revision or mismatch rolls back the snapshot, decision, receipt,
activity, and outbox and returns the typed conflict/refusal. Same-key concurrency waits for and
replays the completed exact response without another snapshot or selection. Cross-key initial
commands serialize on the Conversation lock: one may be fresh; every loser returns the persisted
initial decision conflict/readback and creates no routing rows.

#### Atomic initial admission

Conversation authority admits exactly one Turn with:

- the prospective UUID bound by the selected route;
- sequence 1 and role `user`;
- `possible_side_effects=unknown`;
- status `active` and revision 1; and
- the exact Conversation and new starting RuntimeSession cross-link.

The same transaction inserts exactly one ordinal-0 completed Message history item. The
fresh/replay/refusal result, Turn, history item, exact receipt, activity, and outbox form
one atomic owner result. Exact-key replay is read-only. Every competing key commits no
Turn, history, activity, or outbox effect. Wrong identity, role, sequence, side-effect
state, status, revision, ordinal, kind, second item, or cross-link rejects the transaction.

#### Process, thread, and effect replay

Before ProcessGeneration prepare/spawn, thread fence/start/bind, and ProviderAttempt
prepare/authorize, the effect owner locks the selected Turn and requires it to remain
active revision 1 under the same Conversation and first RuntimeSession.

Immediately before spawn, the runtime also validates the selected working directory under
the runtime-composition contract. This host check cannot select an account, change the
route, authorize a repository, or replace the exact Turn and ProcessGeneration fences.

ProcessGeneration and thread establishment through bind require the applicable
`starting` RuntimeSession revision. ProviderAttempt preparation and authorization require
the exact post-bind `active` revision and exact completed thread fence/bind receipts. A
terminalization race loses before effect execution.

ProcessGeneration has four typed outcomes: `Fresh`, `Replayed`, `Rejected`, and
`Unknown`. Only `Fresh` returns non-clone spawn authority. All other outcomes use durable
readback or refusal and cannot spawn, replace, adopt, create a successor, duplicate an
attempt, or terminalize the Turn. The same rule applies after result loss at every thread
and ProviderAttempt fence.

Conversation authority may transition the exact Turn to `failed` revision 2 under the
starting RuntimeSession only when positive readback proves definite pre-effect refusal.
The proof excludes every process state that may have created a child, every thread
fence/start/bind, and every prepared, authorized, or unknown ProviderAttempt. Ambiguous
work remains active and returns `Unknown` for manual recovery.

Explicit successor remains PostgreSQL-only non-dispatch evidence. It has no protocol
field, product command, runtime execution grant, facade, fallback, or wake path. Before
any write, it locks the Turn named by the route and requires the same Conversation/source
RuntimeSession, status `failed`, and revision 2.

#### Final trigger functions

The latest schema creates these eight final trigger-function bodies and bindings directly:

| Trigger function | Exact Candidate-5 predicate |
| --- | --- |
| `decodex.enforce_routing_completeness()` | Enforce the two selecting snapshot authority shapes, exact Account Registry quota tri-states, reverse nullability, and retained ManagedRun Project-policy `L6` behavior. |
| `decodex.enforce_routing_decision_completeness()` | Require either one initial/ManagedRun selection classification with exact replay exclusions or one non-selecting Conversation continuation binding to the source RuntimeSession and original initial decision; reject mixed fields. |
| `decodex.enforce_runtime_session_state()` | Preserve revision-one insert, identity, timestamps, terminal immutability, legal terminal edges, and ended-session active-Turn rules. Permit only: unfenced `starting` to request-fenced `starting` by setting the complete request ID/digest pair; that exact row to `active` by preserving the request, matching response ID, and setting exact response digest/thread ID; and `active` to `active` only for exact last-Turn acknowledgement. Reject generic `starting` to `active`, partial receipts, combined edges, or unrelated drift. |
| `decodex.enforce_turn_state()` | Preserve active-session behavior. Under `starting`, permit only exact first-Turn insertion and active-revision-1 to failed-revision-2 after positive definite pre-effect refusal. Reject every other starting-session write. |
| `decodex.enforce_history_item_state()` | Preserve active-session behavior. Under `starting`, permit only the admission transaction's ordinal-0 completed Message for the exact first Turn; reject update, second item, other ordinal/kind/status, wrong Turn, or cross-link. |
| `decodex.enforce_provider_attempt_transition()` | Keep the accepted state algebra, unknown reasons, positive terminal-evidence rule, and immutable tuple; include immutable RuntimeSession thread-binding protocol/key identities. |
| `decodex.enforce_provider_attempt_binding()` | For `initial_thread`, require the exact selected route lineage; for later Quick Task Turns, require the exact non-selecting continuation account/profile binding. Preserve ManagedRun and other accepted branches. |
| `decodex.enforce_continuation_plan_completeness()` | For `initial_thread`, require selected `L0` and complete first-session lineage. For same-thread and Context Pack, require the continued decision and unchanged original account/Task RoleProfile snapshots. Explicit-successor completeness still requires the exact failed revision-2 Turn. |

The dispatch-authorization command performs its own exact Turn lock. A deferred trigger
does not replace this pre-write fence. The continuation-rejection helper derives only the
operation from an already-reserved receipt and adds no transport or idempotency mechanism.

No other trigger function changes. Trigger bindings and ACLs remain closed. Every
unrelated write keeps existing active-only semantics. The narrow first-session permissions
are not a generic starting-session bypass.

Candidate 5 adds no module, fixed hierarchy, schema phase, ledger, generic transaction or
recovery framework, transport/idempotency mechanism, wrapper, runner, scheduler, general selector,
capability manager, compatibility bridge, duplicate Quick Task policy path, or explicit-successor
product surface. It must preserve current-main independent account observations, Reset
Card-before-profile ordering within an account, concurrent progress across accounts,
revision-fenced cache publication, and query paths that do not join or start refresh work.

Long-term context consists of immutable Project, Advisor, and Program revisions. Project
context records decisions, constraints, repository facts, active Programs/Objectives,
unresolved risks, and accepted handoffs. Advisor briefs compact cross-project status and
risk. Program context records metrics/signals, recent decisions, quiet periods, and next
review. A Context Pack contains the current revision, recent raw window, relevant
artifacts, and repository instructions/OpenWiki. Summaries never silently replace
sources; users can inspect pinned memory and provenance. The first release uses structured PostgreSQL
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
state sets or clears it. Only `managed_run_project_policy` `L6` uses the Project-era quota
representation with class/duration, remaining amount, reset time, observation time, and
confidence. `conversation_account_registry` does not consume that representation; its exact
duration-keyed 300-minute and 10080-minute slots preserve only the missing, current, or
observation-error `account_quota_facts` fields defined above.

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
reservation. No application, adapter, database, or persistence path rounds or truncates a quota
timestamp. Freshness uses checked integer-microsecond subtraction: an age of exactly 300 seconds
is accepted, while 300 seconds plus one microsecond is stale.

The store owns two canonical mutation schemas: `decodex/quota-window-mutation/2` and
`decodex/quota-exclusion-mutation/2`. Rust constructs them from typed logical values with integer
timestamps, recursively sorted object keys, preserved array order and scalar distinctions, and one
canonical serialization. The receipt binds the resulting SHA-256 digest and byte length, and exact
completed-response replay returns the stored response bytes. Retaining the complete request
document is not required.

The latest schema defines the final typed quota-window and exclusion enums, relations,
constraints, indexes, account foreign keys, observation index, receipts, and authority
directly. There is no old quota shape, conversion, quarantine, drain, backfill, table
replacement, dual schema, compatibility read/write, or hidden fallback. Local pre-release
state is disposable and is never converted into production state.

Separate typed 300-minute and 10080-minute observations remain mandatory. This persistence
boundary enables no account assignment, fallback, `waiting_usage` registration, wake scheduling,
continuation, replay of external effects, or live dispatch. Ingress retains the exact raw provider
timestamp value. Construction of UTC Unix microseconds must be exact; any conversion that would
round or truncate is rejected. Routing Snapshot and Routing Decision consume only exact
values, remain otherwise precision-agnostic, and fail closed. XY-1357 owns the natural
precision evidence after source freeze. Incompatible evidence leaves production routing
disabled and reopens only ingress authority.

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

XY-1400 implements the accepted ProcessGeneration contract in
[the ProcessGeneration authority specification](process-generation-authority.md).
`ProcessSupervisor` is the sole product writer. A private opaque launch authority retains one
protected executable snapshot and derives the durable launch-manifest identity and exact command.
The manifest binds the image and BuildId, fixed `app-server --stdio` arguments, working directory,
sanitized environment, account, initial account revision, canonical credential version and
fingerprint, provider identity, and exact-build startup/lifetime/account-callback capability. No
caller can pair an independent digest with a raw command. The supervisor commits this intent
before a fresh fence can authorize one spawn. Intent, launch manifest, prepare fence, ready
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
effect cancellation, or credential revocation. ProviderAttempt keeps an unproved authorized attempt
`unknown`; process exit or boot change cannot make it `not_submitted`, a replacement cannot replay
it, and any successor is a distinct user-authorized effect with duplicate-risk acknowledgement.
The generic attempt transaction consumes one accepted Routing Decision, Continuation Plan, and ready
ProcessGeneration without creating or changing them. XY-1400 adds no account selection, routing,
ProviderAttempt storage, remote authentication, UI, packaging, release, or live dispatch.
A future live-dispatch protocol gateway must be a separate typed authority that source-rejects
alternate-control RPCs before enablement. XY-1400 does not add that gateway.

### Durable routing and candidate-set authority

PostgreSQL owns revisioned complete Routing Snapshots and immutable Routing Decisions. A selecting
Routing Decision remains the sole account selector; a continued decision binds lineage and cannot
select. A routing request supplies identity and idempotency inputs only. Runtime, protocol clients,
tests, `QuickTaskRuntime`, and
`ServiceApplication` cannot supply or override candidate membership, routing control, sticky
identity, eligibility facts, exclusions, or a preclassified candidate array.

Ordinary initial Quick Task routing uses Account Registry authority directly. The Quick Task
routing adapter is the only transaction owner that may materialize its `L0` Routing Snapshot and
selecting Routing Decision from locked current facts. No later Quick Task Turn enters that path.
The adapter does not require or consult Project routing policy, accepted Project policy,
`routing_compatibility_evidence`, or `quota_windows`. It creates no compatibility bridge or
duplicate policy synchronization authority.

An initial `L0` snapshot binds, under one Account Registry revision boundary:

- every non-tombstoned member exactly once and its exact account revision;
- the canonical routing mode, optional fixed Account UUID, complete account order, and routing
  revision;
- the current Task RoleProfile revision;
- current enabled, lifecycle, health, and credential-binding blockers for every member; and
- exactly one 300-minute and one 10080-minute `account_quota_facts` slot for every member, each
  copied as the exact missing/current/observation-error shape defined above.

Those candidate facts remain in the existing Routing Snapshot/Decision lineage for exact replay
and continuation. They do not acquire legacy revision, remaining, confidence, provenance, or
`quota_windows` values. Project-policy/evidence/build fields are inapplicable and null.

The current `decodex-core::PolicySnapshot` remains a bounded inert value inside one accepted
Project Policy revision. It neither enumerates the account inventory nor establishes routing
completeness, eligibility, evidence provenance, or persistence freshness. For ManagedRun and other
Project-policy `L6` work, Routing Snapshot remains the distinct database-owned complete value. A
Rust wrapper cannot become authorization authority.

A Project-policy `L6` snapshot continues to bind:

- the exact accepted routing Policy revision, versioned `fixed` or `balanced` mode,
  optional fixed Account UUID, and canonical user-owned account order;
- every current account-inventory member exactly once, with an explicit included or excluded
  disposition and an exact account revision;
- sticky affinity plus the exact source RuntimeSession identity and revision;
- required account, RoleProfile, and Codex-build compatibility facts and their exact revisions;
- each exact quota, authentication, capability, and administrative evidence revision used;
- the accepted required-capability set and capability applicability for every member; and
- explicit blocker facts for every unknown or otherwise unusable member.

The two selecting snapshot shapes use a closed discriminator and reverse nullability:
`conversation_account_registry` accepts only the `L0` fields above, while
`managed_run_project_policy` accepts only the retained Project-policy `L6` representation.
Completeness is fail-closed within each shape. A duplicate, omitted, foreign, newly added,
concurrently removed, or revision-changed inventory member, or an unknown required fact, blocks
the snapshot or decision. `managed_run_project_policy` `L6` also rejects an unbound sticky source
or incomplete Project policy/evidence. Silence never means excluded, eligible, or non-applicable.

For Project-policy `L6`, selection and pure quota or reconciliation waits classify only included
members after independent eligibility. Excluded members remain ineligible and do not alter those
wait classifications. `no_route` projects the complete policy-member universe: every excluded
member retains `excluded_by_policy` and its other persisted blockers, and every included member
retains its exact blockers. An all-excluded universe is an explicit cause-complete `no_route`; a
cause-free `no_route` is invalid.

`decodex-core` is a pure deterministic selection kernel over a database-produced selecting
snapshot. It does not establish provenance or completeness and is never invoked for a continued
decision. PostgreSQL atomically persists only the resulting classification and exact normalized
exclusions required for replay. Runtime consumes one exact persisted decision and sequences effects
only; it cannot substitute a decision. Codex is a
positive-evidence capability adapter and cannot determine membership, policy, or eligibility.
One app-server process remains bound to one account, credentials never switch in a live process,
and the separate 300-minute and 10080-minute quota facts for separate accounts are never merged.

Sticky affinity applies only to selecting `managed_run_project_policy` `L6` and wins only when the
bound member is independently eligible under the same complete snapshot. Eligibility requires the
independent versioned `enabled=true` fact; observed health does not imply enablement. Every known
depleted window excludes its account until reset. Unknown, stale, incompatible, disabled,
authentication-failed, missing-duration, low-confidence, or precision-incompatible Project-policy
facts block eligibility.

For initial Quick Task `L0`, fixed mode accepts only the exact fixed eligible member. Balanced mode
uses canonical Account Registry order. Each 300-minute and 10080-minute quota fact is classified
independently from its exact missing, current, or observation-error source shape; neither windows
nor accounts form a merged pool. The persisted result is `selected`, `waiting`, or `no_route`.
`waiting` is manual-retry state in this slice and registers no wake. There is no fallback,
scheduler wake, or re-selection. Policy-bound routing may use only its separately accepted wait
authority; routing itself owns no wake lifecycle.

Account-owned readiness is evaluated only for Project-policy `L6` capabilities explicitly required
by the accepted routing Policy revision. Unknown never satisfies a required capability. If the
accepted required-capability set is empty, unknown account-owned plugin inventory is
non-applicable; it is not positive readiness evidence and does not change an account to ready.
XY-1336 remains future passive-receipt tracking. Host-owned before/after receipts prove causal
no-mutation integrity only and cannot establish account-owned readiness. These capability rules do
not add `routing_compatibility_evidence` to initial Quick Task eligibility.

For a later Quick Task Turn, Routing Authority locks the current RuntimeSession and traces its
immutable lineage to the original `conversation_account_registry` selected decision. It writes
only a `conversation_continuation` decision binding for the prospective Turn. Same-thread and
Context Pack planning consume that binding and retain the exact account and copied Task RoleProfile
snapshots. A selected-account quota, lifecycle, health, credential, or readiness failure returns
typed manual recovery. It cannot call current Project routing, resolve a candidate snapshot, invoke
the selection kernel, fall back, wake, or re-select.

Experiment Authority owns the original causal experiment record. Retained-title Authority owns its
two-effect title bridge. After an exact non-selecting continuation binding with an existing source
RuntimeSession, Continuation Plan owns same-thread continuation when exact positive selected-account
and retained-profile evidence permits it. Otherwise, it owns one atomic Context Pack plus fallback
RuntimeSession on that same account and profile. These owners
serve ordinary Conversation Turns and ManagedRun executions. The stateless ExecutionCoordinator
sequences one Routing Decision, one Continuation Plan, one live ProcessGeneration fence, and
ProviderAttempt preparation. Current production
dispatch stays structurally disabled until its applicable slice gate passes. Ambiguous-turn
replay remains blocked by ProviderAttempt. ManagedRun
consumes the attempt result and keeps only domain lifecycle authority.
Repository/worktree/Git and artifact effects retain their own accepted authorities; routing never
owns or weakens those boundaries.

Those paragraphs define retained final routing authority, not current implementation.
Slice 1 enables only initial Quick Task selection after the Slice-1 subset of
MacDogfoodReady passes. Recovery is an explicit versioned enable/disable, mode, or order
command followed by a new task. It does not rebind or replay a thread.

[XY-1304](https://linear.app/hack-ink/issue/XY-1304) is the later acceptance owner for
automatic cross-account same-thread fallback and all-depleted scheduler wake. Until its
separate reviewed enablement amendment, those paths and automatic Context-Pack fallback
remain hard disabled. It does not block Quick Task, Project/Lead, ManagedRun, GPUI, or
first Mac dogfood. Replay after an ambiguous outcome remains prohibited independent of
XY-1304 and is reconciled by ProviderAttempt.

Missing or observation-error quota slots and unknown, stale, auth-failed, or disabled Account
Registry blockers never imply initial Quick Task eligibility. Capability-unready, incompatible,
missing-duration, low-confidence, and precision-incompatible facts apply only to
`managed_run_project_policy` `L6`. An all-depleted Slice-1 result exposes reset evidence and waits
for explicit retry; it does not schedule a wake.

For Project-policy routing, readiness outside the accepted capability-applicability rule cannot
authorize eligibility, assignment, reassignment, fallback, scheduling, wakeup, continuation, or
production routing. A future operator-triggered active diagnostic requires a separate authority
decision and remains non-routing evidence.

Users exclusively select the four global RoleProfiles through their separate PostgreSQL
authority. Runtime and routing cannot alter or derive model, reasoning, or service tier. Each
RuntimeSession snapshots the current profile for its role; Quick Task requires `task`. Decodex keeps
a user-owned desired inventory only as intent. Until a stable passive account-owned receipt exists,
the first release reports plugin readiness as `unknown`; that unknown is blocking only when a
Project-policy `L6` route requires the corresponding capability, and is non-applicable when the
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

<a id="clean-cutover-and-delivery"></a>

## Local reset and delivery

The local reset has no availability requirement. One reviewed operator-only action stops
the daemon and captures, for every retained account, only Account UUID, enabled state,
account revision, provider binding, credential version/fingerprint, and host-store
binding. It also captures routing revision, mode, fixed target, and the complete account
order. It may replace or directly transform the disposable local database. A recreated
database receives the one empty-target latest-schema bootstrap.

The operator restores or rebinds that exact tuple against unchanged
`HostCredentialStore` records. `fixed` mode requires one non-null target in the retained
account set and complete order; `balanced` mode requires a null target. The order is a
duplicate-free permutation of all retained accounts, including disabled accounts. Exact
readback proves every Account UUID, enabled value, revision, binding, and the routing
mode, target, and order together. The daemon starts only after exact current-authority
and tuple readback pass.

For credential agreement only, the action may invoke the existing
`HostCredentialStore` owner. The owner performs a confined in-process exact read,
recomputes and compares the credential fingerprint and binding, and returns only a typed
credential-negative agreement result. The operator action and result never expose,
serialize, copy, log, persist, rotate, delete, or return token bytes.

The action is not a product account or migration API, manifest, generic attestation
framework, metadata sidecar, bulk importer, generic migrator, state machine,
receipt/finalizer, backup/rollback mechanism, compatibility branch, or fallback. It
retains no quota, usage/profile projection, account history, Codex session, SQLite
execution state, Linear lane, or Codex-created task. Normal startup reads only current
PostgreSQL and `HostCredentialStore` authority. Recreate selected Projects explicitly
from reviewed inventory.

Delivery has exactly three dependencies: Accounts/Quick Task/Accounts-Conversation-Health
GPUI, then the bounded Project/Lead/ManagedRun flow and Project-Work-Run GPUI, then the
two-account self-hosting restart E2E and Mac package. The exact gates and the
MacDogfoodReady-versus-final deferred table are in the
[gate manifest](vnext-gates.md#delivery-slices).

After the Quick Task source freezes, one integration owner must produce the next runtime
candidate. The third runtime-bootstrap candidate is donor source only. The owner
reconciles core configuration; runtime bootstrap, application, library, Quick Task, and
managed-repository modules; protocol doctor, Quick Task, wire, and library surfaces; root
Cargo/task-runner/lock files; deleted storage-spike references; and stale
migration/configuration fixtures. This source cleanup is part of integration acceptance.
Another runtime-lane-only patch is not an accepted candidate.

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
continuity by the Managed Repository Persistence/Executor/Saga owners, and paginated positive GitHub readback by the sealed
GitHub effect boundary. It contributes no unique production behavior to the vNext candidate.

## First-release non-goals

- Pi as a second runtime; per-run/per-agent `CODEX_HOME`; Codex Project sync.
- Linear import, projection, identity, lane authority, or compatibility.
- SQLite product authority, dual writes, or import of historical Codex/SQLite execution state.
- Domain Agents, automatic multi-Lead, arbitrary durable roles, or Goal as general
  planning/review/development state.
- Graph/vector databases, event sourcing, CRDT/DeltaDB worktrees, distributed workers.
- Unauthenticated remote control or runtime-selected model/reasoning/service tier.

## Decision-changing evidence

Only the falsifiers in the owning decision may revise this contract. A failing gate
freezes the affected milestone and records the contradiction; it does not authorize a
silent legacy fallback.

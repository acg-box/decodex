# Decodex vNext Authority Contract

Status: normative target contract; implementation is gate-controlled.

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
  `reviewer_unavailable`, `reviewer_failed`.

The inert V12 boundary persists only `waiting` ManagedRuns that remain blocked. Every run is
foreign-key bound to its exact Project, canonical WorkItem, and authoritative RuntimeSession
revision. Task and Reviewer assignments are exact-run RuntimeSession identities whose closed role
type cannot represent Advisor or Lead and contains no durable Agent identity. Effect lineage is
foreign-key bound to one run and one barrier. Barrier states are `guarded` or permanently `closed`;
both deny effects, and there is no open state or positive execution transition in V12.

The only V12 mutation consumes a positively observed unknown exact turn, a Decodex-owned exact
submitted-turn receipt, or an explicit inconclusive observation. It atomically preserves a blocked
waiting run, records divergence only from positive unknown-turn evidence, closes the barrier once,
and stores exact replay bytes. A stale submitted receipt and an inconclusive observation remain
fail-closed without asserting divergence. Missing, empty, exhausted, not-found, scan-exhaustion,
no-event, or method-result absence is not an input and cannot authorize progress. Experiment
creation, observation production, run creation/acquisition, scheduling, dispatch, progress,
validation, completion, review verdicts, repair, and landing remain outside V12 and blocked by
XY-1304 or later owners.

The safety transaction reserves its exact receipt and validates input before acquiring hierarchy
coordinator 1271, then the run-scoped 1338 lock, before any run/session/barrier/Turn read or lock.
V12 also forward-repairs the V3 Turn and HistoryItem invoker-rights guards for V10's SELECT-only
RuntimeSession authority: they read but never row-lock RuntimeSessions, retain their legal
Conversation and Turn row locks, and accept direct hierarchy DML only in `READ COMMITTED`.
Unsupported isolation fails retryably with `40001`; it never authorizes a stale absence decision.

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
| Credentials | host credential-vault boundary; PostgreSQL stores account metadata/health, never ordinary credential rows |
| v0.2 state | cold backup/tag and historical evidence only; vNext never reads it as runtime input |

PostgreSQL is not event sourced and no graph database is used. Stable IDs plus correlated
activity derive graph/timeline projections. `decodexd` is the sole product scheduler,
app-server child owner, mutation coordinator, and repository-side-effect owner. GPUI,
SwiftUI menubar, CLI, and MCP are clients/adapters over common application services; they
never read PostgreSQL, rollout files, blobs, or repositories directly. V1 is single-host
and has no worker registry or distributed mesh. Remote UI may be added only through the
protocol security gate.

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
Experiment creation and positive observation acquisition belong to XY-1304; production Quick Task
creation belongs to XY-1276. External Codex activity may be provenance-imported for ordinary
Quick/Advisor/Lead conversations; on an active ManagedRun it marks the session `diverged` and blocks
side effects until tool/repository readback reconciles them.

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

Each app-server process is bound to one account. Shared `~/.codex` supplies configuration
and plugins; per-process credentials are never switched under a live runner. Account
state is `unavailable`, `available`, `depleted`, `unknown`, `auth_failed`,
`plugin_unready`, or `disabled`. Each quota window stores its class/duration, remaining amount, reset time,
observation time, and confidence; 5-hour and 7-day windows are never inferred from
positional primary/secondary ordering.

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
continuation, replay of external effects, or live dispatch. Before any live quota ingestion or
routing, XY-1304 must capture the natural app-server/provider timestamp precision. If upstream
timestamps are not exact microseconds, routing remains blocked and the architecture reopens around
PostgreSQL-in-transaction canonicalization; silent rounding or truncation remains forbidden.

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
dependencies. Daemon/host crash can orphan OS descendants; restart reconstructs neither assignment
nor launch authority and requires fresh exact PostgreSQL observations.

Routing honors an available sticky Advisor/Lead account, otherwise chooses an available
compatible account by user policy and quota facts. Every known depleted window excludes
an account until reset; unknown is distinct and receives bounded probe/backoff. When all
accounts are depleted, persist `waiting_usage` and the earliest ready time. Persist the
specific account/window exclusion before rate-limit failover.

Cross-account resume of the same Codex thread is allowed only after the two-account E2E
gate. Otherwise start a new RuntimeSession from a Context Pack while preserving the
Conversation and ManagedRun. Never replay a possibly side-effecting turn without receipt,
worktree/Git, and artifact reconciliation.

Those paragraphs define the target behavior, not current enablement. Until the separate
[XY-1262 live account-routing enablement gate](https://linear.app/hack-ink/issue/XY-1304)
passes and repository authority explicitly enables it, all of the following are hard
default-disabled in every production, dogfood, cutover, and release configuration:

- sticky-account assignment and policy-based account assignment;
- a quota-driven exclusion causing selection or assignment of another account;
- `waiting_usage` scheduling or wakeup;
- automatic same-thread continuation on another account;
- automatic creation or dispatch of a Context-Pack fallback RuntimeSession; and
- replay of a turn after an ambiguous outcome or any possible side effect.

Foundation code may represent these states, persist inert metadata, calculate pure
decisions, and test transactions with synthetic fixtures only where the gate manifest
permits it. It must not submit a turn, assign a fallback runner, schedule a wake, or
transition a live ManagedRun through these paths. Unknown, missing-duration, stale,
low-confidence, auth-failed, plugin-unready, and disabled account/quota facts never imply
availability. In particular, unknown or stale quota is fail-closed: no assignment and no
automatic fallback is permitted; the unavailable/unknown condition must be surfaced for
human resolution or bounded observation.

Readiness cannot authorize account eligibility, assignment, reassignment, fallback,
scheduling, wakeup, continuation, or production routing. A future operator-triggered
active diagnostic requires a separate authority decision and remains non-routing evidence.

Users exclusively select the four global RoleProfiles. Runtime cannot alter model,
reasoning, or service tier. Each RuntimeSession snapshots its profile. Decodex keeps a
user-owned desired inventory only as intent. Until a stable passive account-owned receipt
exists, V1 reports plugin readiness as `unknown` and does not compare host facts into an
account-readiness conclusion or guide mutation from such a conclusion.

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
payload type. Reconnect is snapshot plus cursor-resumed deltas with backpressure. Major
versions match exactly; server supports current and previous minor for UI/server rollout.
Large artifacts use authenticated HTTP, never WebSocket snapshots. Non-loopback binding
remains disabled until authentication, TLS, authorization, and redaction gates pass.

GPUI is the primary workspace and exposes the Advisor inbox; Projects with persistent
Lead Conversations; Quick Tasks; Program/Objective/WorkItem board; Run, review, repair,
and landing state; agent/thread/automation graph and causal timeline; accounts, plugin
readiness (typed `unknown` in the first release), global RoleProfiles, and system health.
Users can always talk to Advisor or a
Project Lead, start Quick Tasks, intervene in WorkItems/ManagedRuns, and inspect all
agent/message/automation relationships. SwiftUI is a thin accounts/run-health menubar
client over the restricted protocol. GPUI caches are bounded, disposable,
cursor-paginated, and keyed by server/schema/content hash; project opening never eagerly
loads all history.

## Migration and delivery

Cutover has no availability requirement. Stop v0.2, tag the trusted `main`, and preserve
cold copies of old SQLite/config/automation inventory plus incident scenarios. Start
vNext with empty PostgreSQL product state. Do not import old Codex sessions, SQLite
execution state, Linear lanes, or Codex-created tasks. Recreate Projects and Automations
explicitly from reviewed inventory.

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

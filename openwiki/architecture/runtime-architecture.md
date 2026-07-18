# Runtime Architecture

This page explains the active vNext ownership skeleton and preserves a map of the
excluded v0.2 source for provenance. Checked-in manifests, source, tests, and the vNext
authority documents remain authoritative.

## Workspace shape

The root manifest enumerates the active members explicitly. Five library owners form the
vNext runtime boundary:

- `decodex-core`: domain/application contracts and ports, including logical
  Conversation/RuntimeSession/history and deterministic Context Pack compilation, plus the XY-1306 typed
  `~/.decodex` path, configuration, stable-identity, blob, and disposable-cache
  foundation. Its external dependency set is architecture-tested and limited to
  bounded TOML/Serde parsing, SHA-256, OS randomness/no-follow filesystem support, and
  test-only temporary storage.
- `decodex-protocol`: V1 typed wire contracts, current/previous-minor negotiation, and
  loopback endpoint policy; depends only on core plus structured serialization.
- `decodex-postgres`: the PostgreSQL 18 product-state adapter; depends on core plus the
  accepted tokio-postgres/deadpool/refinery stack and owns embedded migrations,
  optimistic transactions, leases, append-only activity, outbox delivery, inert
  account/window metadata, normalized history, immutable session snapshots, blob metadata,
  Context Pack revisions, and inert transition proposals.
- `decodex-codex`: typed app-server contracts, schema/live capability negotiation, and
  redacted event normalization. It depends only on core, performs no SQL, owns no database
  connection, and exposes no child-launch surface. Live turn dispatch remains unavailable
  while XY-1304 is failed.
- `decodex-runtime`: service lifecycle, connection/session execution, resumable event
  publication, idempotency receipts, private immutable-account process supervision, and the
  sole PostgreSQL/Codex adapter composition; depends on the other four owners plus the
  maintained Axum/Tokio transport stack.

`apps/decodexd` depends only on runtime. The `apps/decodex-cli` and
`apps/decodex-gpui` client roots depend only on protocol, so they cannot reach stores,
Codex, repositories, or orchestration directly. Radar and Publisher remain independent
auxiliary workspace members. `tests/scripts/test_vnext_architecture.py` checks the exact
dependency graph and exclusion of the legacy package through Cargo metadata.

`decodex-protocol` owns the reusable bounded WebSocket client alongside the shared wire
contract. It reads only the client projection of typed configuration: profile data is
validated, while server-host repositories, PostgreSQL data, and cache policy are consumed
as opaque TOML and never represented by the client profile. A local profile uses its
explicit identity pin or the shared-host stable identity file; a remote profile requires
its explicit pin and carries only host and port. The client sends a pinned V1.2 hello,
verifies welcome and snapshot version/identity, issues `get_doctor_status`, and re-verifies
the result, embedded report, and exact complete current component set before returning status.
Report ordering is not authority. Reads, writes, frames, messages,
interleaved events, and deadlines are bounded; socket, parser, HTTP, and server-provided
text collapse into closed redacted failure classes.

`apps/decodex-cli` exposes the canonical `status` and `doctor` commands with active or
`--profile NAME` selection and human or `--output json` rendering. Both commands cross the
same V1.2 query; `status` is compact and `doctor` is line-oriented, while each retains every
typed check. JSON uses `decodex/cli-diagnostics/1`. Exit code 0 means every check is ready,
1 means a complete report contains unavailable or unknown checks, and 2 means a closed
client/configuration/protocol failure. The CLI has no mutation command or infrastructure
dependency.

`decodexd` is the only V1 server composition root. It binds
`ws://127.0.0.1:49152/v1/ws`, and the endpoint type refuses every non-loopback address
before opening a socket. The single physical WebSocket uses structured JSON and typed
hello, command, receipt, result, snapshot, event, and refusal envelopes. Major versions
must match exactly; this build accepts minors 2 and 1. Events carry server ID, monotonic
cursor, entity revision, correlation, and causation. The stable server-host ID supports
operator pinning, while each daemon process creates a distinct bounded publication-epoch
ID. A reconnect resumes retained ordered deltas only when both IDs match; an absent or
changed epoch, stale cursor, or changed server ID receives a bounded snapshot fallback.
Only a snapshot or event cursor fully applied by the client is a resume checkpoint; the
Welcome cursor is an informational server high-water mark and must not advance client
progress before following replay deltas are applied.

Runtime receipt lookup is keyed by the negotiated protocol version and command idempotency key;
the stored request fingerprint additionally covers that version, typed payload, and optional
expected revision. A same-version duplicate returns the original command identity and stored
result without a second application execution. Reusing a mutation key across V1.1/V1.2 executes
once in each version namespace and retains each version's native command outcome; neither outcome
replays, conflicts with, or poisons the other namespace. Other same-version conflicting reuse is
rejected.
Each negotiated protocol-version namespace has its own fixed lifetime receipt capacity, so one
minor version cannot consume another version's slots. Total memory remains bounded by the finite
supported-version window times that per-version capacity. Accepted keys are never evicted,
duplicates remain readable at capacity, and new same-version keys are refused before
application execution once full. Replay buffers, snapshot item counts, human-readable
wire scalars, inbound and outbound message sizes, writes, and per-client outbound queues
are bounded. Queue overflow disconnects that client with WebSocket close code 1013;
an oversized outbound frame closes with code 1009. A successful application publication
is retained even if its initiating client overflows, so cursor resume cannot lose the
mutation. Once accepted, command execution is shielded from connection-task cancellation;
a disconnect can suppress delivery but cannot cancel receipt/event recording. Snapshot
types contain only small state and cannot carry artifact bytes.

This slice deliberately keeps its replay/idempotency ledger in memory. A daemon restart
loses that transient ledger; the stable server-host identity remains persisted, while a
fresh publication-epoch ID makes every old cursor fall back even when restarted cursor
values are equal or overlap. Durable PostgreSQL product-state and transaction primitives
live in `decodex-postgres`.
`decodexd` loads only the typed `~/.decodex/config.toml` and passes its explicit Unix-socket
directory, port, database, operator-pinned expected PostgreSQL peer UID, and distinct
migration/runtime identities with independently resolved optional credentials to that adapter.
The adapter opens each directory component relative to a retained descriptor, pins the directory
and socket device/inode identities, requires the final directory and socket to be owned by the
configured UID with no group/other directory write access, and verifies the connected kernel peer
UID before PostgreSQL authentication starts. It repeats path binding and peer verification for
every migration and runtime-pool connection, so a pre-planted endpoint cannot select its own trust
root and later ancestor or endpoint replacement cannot redirect credentials. A single-use migration pool performs only
forward migration and migration verification and is closed before the runtime pool is built.
The adapter reports available only after PostgreSQL 18, checksum, pgcrypto, immutable migration,
two-connection pool checks, and an exact steady-state authority audit pass. The audit starts from
the original `session_user` and evaluates both its immediately effective authority and every role
reachable through `SET ROLE`, including NOINHERIT/SET-only chains; membership admin option is
itself unsafe because it could create a new SET path. Effective ownership is one closed PostgreSQL
18 inventory covering the Decodex schema, relations, functions, types, collations, conversions,
operators, operator classes/families, extended statistics, and text-search
configurations/dictionaries. Extension control is audited separately from extension schema:
`pg_depend` extension-membership edges for every Decodex object and dependent execution object,
plus extension members referenced by those objects, are unsafe whenever the extension owner is
runtime-effective, regardless of `pg_extension.extnamespace`.
Superuser/BYPASSRLS/role/database administration, database/schema CREATE,
TRUNCATE/TRIGGER/REFERENCES/MAINTAIN, excess table DML or grant options,
`session_replication_role` SET/ALTER SYSTEM, and any effective non-`origin` login value are unsafe
in any reachable authority state. At the V14 boundary, the audit verifies all 110 shipped
non-internal trigger bindings, including regular and deferred constraint triggers, by table, event
mask, row/statement level, constraint and deferral state, origin-enabled mode, and function binding,
then compares
each bound function's exact metadata and `pg_proc.prosrc` bytes with the canonical body embedded in
the immutable forward migration ledger through V14. It additionally closes the entire runtime-callable `decodex` function
namespace over exact signatures and overloads, argument/result shape, language, volatility,
parallel/strict/set behavior, planner metadata, exact security-invoker/definer state and exact per-function settings,
and canonical source. Unexpected functions, overloads, owner-executed functions, or unsafe settings
are unsafe; missing functions or noncanonical source are incompatible. Disabled or misbound triggers
are unsafe; a replaced same-signature safety-function body is incompatible.
Every non-internal trigger on a Decodex runtime relation must be one of those 110 exact V14 bindings.
The same closed execution-path audit permits no user rule, row-security policy, or enabled/forced RLS
on those relations and rejects non-`pg_catalog` function/operator dependencies from defaults,
generated expressions, constraints, indexes, rules, or policies unless they resolve to one of the
119 canonical V14 functions. Every canonical function has the exact function-local
`pg_catalog, decodex` search path, so runtime-selected callable or operator shadows cannot redirect
trigger or constraint execution. A trigger cannot therefore invoke an adjacent public owner-executed
function merely because runtime DML fires it.
One version-specific canonical PostgreSQL 18 manifest additionally closes all Decodex relations,
columns, defaults, constraints, indexes, enum labels, and internally generated constraint triggers.
Defaults, constraints, indexes, and internal triggers include their exact stable catalog dependency
identities rather than raw OIDs.
Constraint inventory covers both `conrelid` in Decodex and external constraints whose `confrelid`
references Decodex. Internal trigger identity is tied to the exact canonical constraint, relation
side, trigger function, event semantics, deferral state, and referenced relation/index rather than
generated trigger names or OIDs.
`public.refinery_schema_history` is always schema-qualified and must have
exactly table SELECT. Ownership, SET-reachable authority, table or column grant option, writes,
TRUNCATE, REFERENCES, TRIGGER, and MAINTAIN are unsafe; missing SELECT is incompatible before the
history row query runs. The ordered ledger must exactly match every embedded migration version,
name, and checksum; missing, extra, duplicate, reordered, or tampered identity is incompatible.
The three bound identity sequences must be exact. Runtime receives USAGE only on the activity and
outbox sequences; the migration-owned history-version sequence remains inaccessible. SELECT,
UPDATE/`setval`, ownership, grant options, and SET-reachable surplus authority are unsafe. Every
string-to-system-catalog identity explicitly qualifies `pg_catalog`; the
authority audit and schema-qualified migration-ledger verification remain correct under a hostile
runtime `search_path` that shadows both ledger and system-catalog names. Missing required schema,
table, sequence, function, or ledger-read authority is incompatible.
At the V14 boundary, twenty-seven canonical `SECURITY DEFINER` functions comprise the three V3
cursor/history functions, eleven V5-V7 Project/Policy/Program/Objective command entrypoints, and two
V9 RoleProfile command entrypoints, two V10 RuntimeSession command entrypoints, and four V11 exact
WorkItem commands, the inert future running/resume guard, the one V12 ManagedRun safety
consumer, and the three V14 routing-authority commands. The cursor issuer derives Conversation,
snapshot version, parent, page size, position, item identity, and expiry under serialized
Conversation authority; the bounded pruner is callable by runtime, while the capture function is
trigger-only and runtime cannot execute it directly. Runtime has no cursor-table INSERT authority.
The other ninety-two canonical V14 functions are security invokers. The additional-function adversarial fixture creates a fixture-only migration-owned,
runtime-executable `SECURITY DEFINER` function with an unsafe per-function setting and migration-owner
trigger authority, proves runtime direct trigger DDL is denied, executes the owner-authority effect,
and restores the trigger before the independent doctor rejection. A separate public-function trigger
fixture proves runtime DML can execute an owner effect without direct function `EXECUTE`, protected
table `UPDATE`, or `TRIGGER`; the closed V14 trigger inventory rejects that path. A public,
runtime-owned extension fixture attaches a migration-owned Decodex collation as an extension member,
proves the runtime can transactionally drop it, and is rejected through the dependency audit. The
closed 119-function V14 inventory remains independent of the distinct same-signature canonical-source
substitution fixture. Missing, malformed, unsafe, unreachable,
authentication-failed, or incompatible bootstrap retains a typed unavailable adapter;
there is no ambient/default database or alternate state authority. Repository and
PostgreSQL socket validation rejects a symbolic link or non-directory at every descriptor-opened
component, a non-socket endpoint, untrusted directory permissions/ownership, an endpoint owner or
kernel peer that differs from the operator UID pin, and any directory/socket identity replacement.

Protocol V1.2 adds `get_doctor_status` as a read-only query/result with a client query identity and
no mutation receipt, deduplication, replay, receipt-capacity use, event publication, or entity
revision. Reusing a query identity performs a new ordered observation. V1.1 remains
the rolling previous minor, receives only its existing wire shapes, and safely falls back
to a snapshot because it cannot present an epoch ID. `ClientHello` may pin the stable
server identity before snapshot, query, or command access. The doctor report is mechanically
capped at 32 unique typed checks and has no free-form external text. Server repository
paths are an aggregate typed check only, so
remote clients receive neither host paths nor repository names and cannot reinterpret
them locally. App-server capabilities are closed enum values; current unprobed capability,
plugin, vault, and blob-content observations remain honestly `unknown` rather than ready.
Each V1.2 doctor read revalidates the retained socket binding, obtains a runtime connection through
the verified connector, performs a live query, reruns the complete runtime-authority and immutable
migration checks—including the exact embedded ledger and required `pgcrypto` extension—and reports
any failure as typed unavailable. It never reconnects migration
credentials, runs migration, or repins an endpoint. A stale but securely bound listener refusal is
database-unreachable; directory/socket replacement or peer-identity drift is unsafe-host-path.
PostgreSQL socket recreation after restart therefore requires a daemon restart under the explicit
operator authority instead of silently adopting the replacement.

The XY-1273 account foundation keeps a canonical Decodex account UUID and a closed readiness
observation in core. PostgreSQL persists only display metadata, that observation, ordinary
credential-negative JSON, and revision evidence; the forward-only V4 migration adds the honest
`unavailable` observation without adding any credential, vault-reference, selector, or routing
column. The repository exposes only exact-ID reads and inert mutations.

Codex child creation has one dormant runtime-owned explicit manual composition path. Runtime first
observes the exact manually selected PostgreSQL account ID/revision in the `available` state, then
releases the result row and pooled client before reserving process capacity or invoking a vault. It
repeats the same exact observation after process-group cleanup or quarantine transfer and constructs
only a non-live post-cleanup result. Readiness may change while mechanics run; a stale, non-ready, or
unavailable final observation suppresses the result. No transaction, row lock, client checkout, or
caller callback spans vault or process work. A synchronously blocked host vault can still retain its
local task and mechanical capacity indefinitely, outside PostgreSQL.

The manual launcher, request, result, capacity counter, permit, vault port, wire DTOs, stdout pump,
and process supervisor are private to non-reexported runtime modules. The Codex adapter does not
depend on PostgreSQL and exposes no child-launch operation.
Cargo-metadata architecture guards enumerate every workspace library and binary target and prove the
current normal-dependency graph: only runtime directly owns both sibling adapters, while `decodexd`
reaches them only through runtime; production edges do not enable synthetic fixture features.
Compile-fail contracts prove that the private launcher, capacity, command, probe, and vault types are
absent from current crate APIs. Metadata does not prove the absence of future source changes or
wrappers. The private account binding and host vault project into the not-yet-bound child exactly
once, after which repeated `account/read` observations compare the exact identity in zeroizing,
redacted process memory and attest the exact process ID. No account-identity digest is returned.
A mismatch synchronously terminates the process group or transfers cleanup to the bounded quarantine;
uncertain cleanup never returns a runner. The default vault is unavailable. Children retain the
single normal shared `~/.codex` for configuration and plugins, receive only `HOME` and a fixed
`PATH` from the parent, mark all other inherited descriptors close-on-exec, use a fixed app-server
argv, and discard stderr. Outbound JSON serialization writes directly into fixed 8 KiB
zeroizing blocks; the pointer-only block index may grow, but no growing ordinary allocation ever
owns credential bytes. Serialization failure, frame overflow, transport failure, and normal
teardown drop the same wiping owners. Inbound reads,
partial frames, queued frames, overflow/disconnect values, and unread teardown state remain in
fixed-allocation zeroizing blocks; the exact-size contiguous parse copy is zeroizing. A lexical gate
rejects every escaped inbound JSON string before locked serde_json 1.0.150 can copy it into ordinary
scratch; this intentionally closed subset fails unusual escaped paths/messages closed. Every completed
owned string field under the typed RPC DTOs deserializes directly into a per-field zeroizing owner,
so a later missing, malformed, nested, or wrong-type field cannot ordinarily free it. This guarantee
does not claim that all opaque Serde structural/number scratch is zeroizing, only that inbound string
bytes cannot enter its escaped-string scratch. No `CODEX_HOME`,
credential file, global configuration mutation, ambient-account production probe, or live credential
switch is exposed.

Runner capacity is fixed at 64 and in-memory under one daemon-owned runtime authority. Reserving a
private non-clone permit also reserves one of 64 fixed cleanup slots before spawn. The permit enters
every version/schema preflight `ProcessGroupOwner` immediately after successful
`spawn`, before any fallible post-spawn step. It returns to the launch attempt only after confirmed
process-group absence and successful child reaping, then moves sequentially into the next preflight and
final app-server. Confirmed final shutdown releases it only after the same proof; uncertain cleanup
moves it into a hard-capped quarantine before control returns. The one capacity-lifecycle janitor must
be successfully created before capacity authority exists; failure makes capacity construction fail
closed, so no child can require later launch-triggered recovery. No caller or `Drop` path enters the
background retry loop. A weak daemon registry reuses exactly one live capacity authority but does not
make it process-immortal. A finite join coordinator stops and joins the janitor when the last capacity,
permit, or quarantined job releases that authority; if the last release occurs on the janitor itself,
the coordinator performs the join without self-wait. Construction failure after worker start shuts
down and joins that worker before returning. The janitor scans the fixed slots
round-robin and performs one nonblocking cleanup attempt per job per round, so one stuck group cannot
starve later groups. Atomic slot states keep each job discoverable while reserved, ready, or in
flight; an in-flight guard restores the same slot after unwind and a per-iteration unwind boundary
keeps the persistent owner alive. Poison is recovered, and timed predicate rechecks prevent lost
wakeup. There is no queue-full or
contended-admission leak: the 65th permit is rejected before spawn. The failed attempt cannot launch
another group. PostgreSQL's
exact revision/state predicate excludes stale, unavailable, unknown, depleted,
authentication-failed, plugin-unready, and disabled observations. A fresh daemon starts with no
persisted capacity or assignment authority, but uncatchable daemon/host termination can orphan an OS
process group because the in-memory quarantine is not an external process supervisor. Restart never
adopts such a group or recreates authority; a later observation requires
fresh exact PostgreSQL pre- and post-observations for the same manually selected account. There is no account
inventory, automatic selector, weighting, stickiness, fallback, quota wake, or live routing API;
XY-1304 remains the separate failed dispatch gate.

The accepted XY-1355 target adds no live execution path here. V14 makes PostgreSQL the sole owner
of a revisioned complete routing-policy snapshot over the entire account inventory, canonical
user order, explicit per-member disposition, sticky affinity plus source RuntimeSession revision,
account/profile/build compatibility, exact evidence revisions, and required-capability
applicability. Runtime and callers cannot supply that universe. V14 exposes only exact policy
replacement, ordinary compatibility-evidence publication, and immutable snapshot-resolution
commands; resolution classifies facts and blockers but cannot select, wait, wake, continue, or
dispatch. The core routing component will be
a pure kernel over the database-produced snapshot; V16 will atomically persist its decision and
complete evidence linkage. Codex remains a positive-evidence adapter. An unknown required
capability blocks, while an empty required-capability set makes unknown plugin inventory non-
applicable rather than positive readiness evidence.

V1.2 also carries `get_conversation_history`. Its request contains a logical Conversation UUID,
an optional opaque PostgreSQL-issued Conversation-bound snapshot cursor, and a page size capped at
eight on the wire (the repository's internal cap is 100). PostgreSQL assigns append-only
per-Conversation positions and derives the next position and snapshot high-water from indexed
history while holding the Conversation lock; there is no runtime-writable stored counter. The first
page also pins the current append-only history-version sequence. Later pages fetch only `limit + 1`
immutable item versions at or before that sequence and through the high-water position. Concurrent
appends are neither duplicated nor silently skipped, and later streaming updates cannot change an
issued page or its replay.

Every continuation is a random identifier resolved only through an immutable persisted issuance row
that binds Conversation, positive high-water, immutable version sequence, fixed page size, positive
item position, exact item identity, and optional issued parent. The canonical issuer derives and
extends the chain under serialized authority; ordinary runtime DML cannot mint one. A page exposes
at most one continuation. Chains expire after one hour and are capped at 512 rows per Conversation
and 4,096 globally; canonical retries reuse an existing row, while new issuance at capacity returns
typed `resource_exhausted`. Bounded pruning removes expired chains and obsolete history versions but
retains current versions, active-cursor versions, and versions required for exact command replay.
Never-issued, expired, cross-Conversation, edited, zero-position, changed-page-size, and forged
truncation tokens fail closed.

Each command first commits an immutable pending receipt containing protocol version, operation,
project and scope identity, entity identity, idempotency key, canonical request digest, expected
revision, and every payload hash and length. The receipt owns a fenced claim token and finite expiry.
Conflicting reuse fails before filesystem or metadata effects; exact completed replay returns the
stored original response bytes, and the first successful caller decodes that same stored response
rather than rereading mutable current state after commit. An expired claim can be reassigned, but a stale token cannot complete
it. Blob-backed writers then acquire sorted session-level hash locks in namespace 1273 on dedicated
non-pooled connections, followed by sorted per-shard capacity locks in namespace 1274. After byte
publication, transaction B acquires hierarchy key 1271 and required parent/child rows, registers
metadata/references/activity/outbox, persists the exact response, and completes the fenced receipt
atomically. Database triggers never acquire hash or shard locks. Cancellation or uncertain unlock
closes the dedicated session and releases its locks.

Cursor paths use hierarchy 1271, cursor key 1272, then rows; they never acquire hash locks in that
transaction. Garbage collection uses hash then shard coordination, rechecks every live-reference
table and grace age in a short transaction, commits metadata deletion, and unlinks afterward.
Filesystem publication, verification, scans, and unlinking occur outside hierarchy, cursor, and
database transactions. `decodexd`, its daemon-private runtime identity, and BlobStore access are one
trusted service boundary; arbitrary/manual use of that credential is unsupported and equivalent to
daemon compromise. PostgreSQL owns committed metadata/domain/receipt authority but does not itself
attest external bytes.

Inline text is capped at 16 KiB. Larger payloads are published create-only and atomically, fully
hash/length verified, and synchronized with their containing directory before transaction B. Direct
and transitive bytes are reverified on every successful read. Bounded grace-aged inventory
reclamation is deterministic and resumable and removes only hashes absent from live history,
immutable replay versions, Artifact revisions, and Context Packs. A crash can leave only
inventory-visible unreferenced bytes; metadata deletion commits before orphan unlink. Missing,
length-mismatched, or tampered content fails closed.

The typed history projection carries one canonical media type for both inline and offloaded payloads
plus a flat credential-negative metadata map. It accepts at most 32 fields, 64 UTF-8 bytes per key,
and only booleans or strings of at most 256 UTF-8 bytes; nested or credential-shaped forms fail
before persistence or wire decode. Core owns both opaque types and their serde validation.
Credential-bearing normalized key suffixes and concrete authorization/token/assignment/key patterns
fail closed, while benign prose such as `secret sauce`, `token budget`, and `session summary` remains
valid. PostgreSQL's closed projection predicate mirrors that classification, so store-created rows
cannot become unreadable at the protocol boundary. This is normalized provenance, not raw app-server
JSON.

V3 state triggers author creation/update timestamps at persistence. Starting or active
RuntimeSessions always begin with `ended_at = NULL`, active Turns always begin with
`completed_at = NULL`, and terminal RuntimeSession or Turn states cannot be inserted. Immutable
snapshot, blob-verification, Artifact-revision, Context-Pack, and transition-proposal creation times
are database-authored rather than caller-authored.

Logical Conversation identity has no Codex-thread or account component. One Conversation can have
multiple explicitly manual or synthetic RuntimeSessions, each bound to immutable non-secret account
and profile snapshots. A RuntimeSession's Codex-thread and last-known-turn correlations are also
immutable across every lifecycle transition. Composite foreign keys prevent sessions, Context Packs, transition proposals,
history items, and typed Artifact revisions from crossing that boundary. History-item Artifact
identity/revision correlation is immutable after insertion and an update carrying a different valid
reference fails rather than reporting an ignored mutation. Each Artifact parent revision is tied by
a deferred composite foreign key to an exact immutable revision row. Before every parent advance,
the state guard requires the exact old revision row; every new revision requires revision 1 or the
exact immutable predecessor plus a legal predecessor-state transition. Typed and direct-SQL
transactions therefore cannot skip history, commit a parent advance without its matching revision,
or manufacture a revision the parent has not selected.
Database triggers take
conflicting parent row locks in child-write and terminal-transition directions, enforcing parent
eligibility and terminal immutability under concurrent runtime SQL. History-item create/update,
activity, outbox evidence, exact response, and fenced receipt completion are one transaction B after
the pending receipt and any blob publication. Context Packs separate the mandatory
pinned revision, use canonical length-delimited binary encoding, bind policy/Conversation/side
effects/complete provenance/bytes/digests in one opaque verified value, and reconstruct that complete
immutable value on read. Source rows are staged before the parent under a transaction advisory lock;
the parent seals their exact contiguous count, and database triggers reject subsequent source or pack
insert/update/delete poisoning while the deferred parent reference prevents orphan commits. Persisted
rollover/fallback rows are proposals only: their schema
forces dispatch disabled. This slice exposes no account selection, live turn start/resume/steer,
automatic rollover, Context-Pack dispatch, ambiguous replay, or scheduler wake; XY-1304 remains the
separate failed enablement gate. XY-1358/V15 owns experiment creation and positive-only observation
authority; XY-1360 owns continuation and atomic Context-Pack fallback; XY-1362 owns scheduler wake;
XY-1276 owns production Quick Task creation. XY-1272 owns only PostgreSQL
configured-principal and ACL authority closure against V8. XY-1345 owns accepted exact-command
authority/prototype evidence, XY-1346 owns expected V9 exact receipts plus RoleProfiles, and
re-bounded XY-1337 owns expected V10 RuntimeSession snapshots and transitions.

V15 is a persistence protocol, not an execution composition root. PostgreSQL first records immutable
intent, then a one-way `creation_possible` fence before any `thread/start` could occur. Only the exact
typed successful response for that same attempt can bind one globally unique thread. A retained
`creation_possible` row after a crash or lost response is terminal ambiguous creation authority:
stored response bytes remain replayable for audit, but the PostgreSQL adapter reports that replay
only as ambiguous readback and cannot reconstruct its private one-shot fresh permission. There is
no transition, Rust API, or Codex API here that retries, searches for, or adopts a thread.
Observations are append-only positive exact facts bound by experiment revision, thread, deterministic
marker, source identity, and payload digest. List omission, pagination exhaustion, missing events,
lossy history, and stale caches have no persisted representation and authorize nothing.

The composition also reports conversation execution unavailable. Authentication, TLS,
remote binding, HTTP artifact transfer, MCP, scheduling, live Codex execution, mutating
CLI operations, and GPUI behavior remain disabled and belong to later issues.

The synchronous product-state availability port reflects verified configuration and local
pool lifecycle: closing the pool makes it unavailable. Live PostgreSQL failures remain
authoritative at each asynchronous store operation; availability does not claim a synchronous
network-liveness probe.

Pure database commands do not use that split-phase flow. They execute through an
operation-specific, command-complete migration-owner `SECURITY DEFINER` function. PostgreSQL builds
the complete JSONB request from the typed values it consumes, inserts or waits on
`exact_command_receipts(protocol_version, idempotency_key)`, and completes the receipt, domain
mutation, canonical activity, outbox, and stored response in one transaction. Runtime has only the
operation-function `EXECUTE` grant: it has no exact-receipt table privilege, private-helper access,
or canonical activity/outbox mutation authority. A deferred constraint trigger rejects any commit
with an executing row; completed rows and authoritative response bytes are immutable and
undeletable. Stable domain rejection completes and replays, while cancellation, connection,
serialization, deadlock, and unexpected database failures roll back.

Normal exact-command execution is one command per top-level `READ COMMITTED` transaction. A
separate read/lock follows `ON CONFLICT DO NOTHING`; `40001` and `40P01` retry the complete
identical transaction. JSONB identity uses equality, explicit null keys, typed enum/numeric values,
and exact PostgreSQL text semantics. Effects use actual `RETURNING` rows and canonical audit
identities. Operation is part of the envelope, so cross-operation key reuse conflicts. The accepted
proof and vertical ownership are in
[the XY-1345 evidence](../evidence/xy-1345-exact-command-authority.md). V9 implements this boundary
for immutable global RoleProfiles through `bootstrap_role_profiles_exact` and
`update_role_profile_exact`; V10 extends it through `create_runtime_session_exact` and
`transition_runtime_session_exact` without changing the V9 receipt or RoleProfile authority.

The V9 RoleProfile model contains only advisor, lead, task, and reviewer. Bootstrap receives four
role-implied scalar configuration groups and commits the complete set or nothing. Updates append an
immutable revision under an expected-revision row lock and atomically advance the selected role's
single current pointer. Runtime neither selects nor mutates the RoleProfile or exact-receipt
relations directly; it parses the response bytes returned by the two command-complete entrypoints
and retries a complete top-level transaction only after a classified infrastructure SQLSTATE.

V10 is a forward-only zero-state cutover over the retained V3 snapshot and RuntimeSession table
identities. One access-exclusive fence rejects every legacy RuntimeSession receipt, snapshot,
session, Turn, or structurally classified activity/outbox row before altering the empty tables.
Classification closes aggregate, event, effect, link, and payload representations recursively,
including legacy `runtime_session_recorded` events under another aggregate and nested aggregate
markers; the steady-state trigger applies the same fail-closed family.
Creation accepts only the RuntimeSession and Conversation identities, one role, the complete
non-secret account snapshot identity/facts, nullable Codex thread identity, and initial state.
PostgreSQL acquires hierarchy coordinator 1271 before selecting or locking an open Conversation or
RuntimeSession tuple, then resolves exactly one current immutable RoleProfile
revision, inserts or equality-validates the account snapshot, writes the complete profile snapshot,
creates revision 1 with a null last-known-turn identity, appends canonical activity/outbox rows,
and stores the exact response bytes in one transaction. Transition identity is only RuntimeSession
identity, expected revision, and target state; success returns prior/new state and revision plus the
unchanged full profile/account/session facts and canonical audit identities. Missing, duplicate,
stale, illegal, invalid-account, and account-snapshot-conflict outcomes are committed stable
rejections. Runtime retains SELECT-only snapshot/session readback and can execute only the two
public command owners; direct DML, private helpers, and forged RuntimeSession activity/outbox
namespaces are closed.

V12 rolls the V3 invoker-rights Turn and HistoryItem state guards forward after V10 made
RuntimeSessions SELECT-only for runtime. Their statement-level `BEFORE` guards acquire hierarchy
coordinator 1271 before row-trigger reads; the Turn guard retains only its Conversation row lock,
and the HistoryItem guard retains only its Conversation and Turn row locks while reading the exact
RuntimeSession without locking it. Both direct hierarchy paths require `READ COMMITTED` and fail
retryably with `40001` under any other isolation level. The ManagedRun safety owner follows the
same global order: reserve the exact receipt, validate the request, acquire 1271, acquire the
run-scoped `(1338, hash(run))` lock, and only then read or lock hierarchy, run, session, barrier,
receipt, or Turn state. This prevents an unknown-turn absence decision from crossing a concurrent
same-session Turn insertion without granting runtime `UPDATE` on RuntimeSessions.

### Managed repository stage-two authority

PostgreSQL is the durable managed-repository authority. It owns the current projection,
monotonic generation/tip, globally immutable complete-descriptor operation assignments,
append-only authority transitions and operation evidence, exact generation/tip compare-and-swap,
atomic command completeness, and state loaded after restart. The pure values, facts, descriptors,
and deciders in `decodex-core` are mechanism-neutral and non-authoritative; a caller projection,
snapshot, operation view, or generic observation cannot substitute for a transaction-internal
PostgreSQL load.

One repository operation ID spans every repository and operation kind. Complete canonical
descriptor equality returns `ExistingExact(OperationView, NoDispatch)`; any difference is
permanent `OperationIdConflict`. For a new assignment, one top-level transaction loads and locks
current authority, evaluates the pure decision, inserts the immutable assignment, appends
`PossiblyEffected`, fences the allocation or head, appends the authority transition, and advances
the projection by exact generation/tip compare-and-swap. Commit-time completeness prevents a
partial durable command.

The PostgreSQL adapter may retain a private non-executable preparation seed until COMMIT. Only a
successful COMMIT acknowledgement returning on that same live control path can turn the seed into
one fresh affine receipt. The receipt cannot be persisted, queried, cloned, publicly constructed,
or reconstructed. Persistence, readback, exact repeat, restart, terminal state, and unknown COMMIT
outcome never grant dispatch. If the COMMIT outcome is unknown, the invocation produces no receipt
and performs no external execution.

Allocate is PostgreSQL-only; all admission, path-reacquisition, stat, Git, and target-availability
evidence used before it is strictly read-only. `Register`, `WorktreeReady`, and `Commit` are distinct
durably fenced `PossiblyEffected` external operations. `Register` uses the accepted pinned Git
2.54 worktree-add mechanism and completes only on exact reciprocal registration with unchanged
head. `WorktreeReady` completes only with its exact head unchanged. `Commit` consumes exact head `H`
and completes only after positive readback of one exact advance to canonical `H-prime`. Restart can
issue only operation-specific readback; it cannot retry, replay, adopt, repair, import, or
reconstruct execution authority.

Accepted XY-1354 supplies descriptor-assisted, symlink-free persisted absolute-path reacquisition
and pinned Git 2.54 unchanged. `decodexd` remains the sole repository-effect owner inside the
trusted single-daemon/same-UID V1 boundary. Authorized whole-cluster restore is inside the trusted
PostgreSQL-administrator boundary and may redefine authority; V1 has no automatic full-cluster
rollback detection. XY-1349 solely owns V13 persistence, XY-1350 owns only read-only acquisition
and executor/readback mechanics against this contract, and XY-1351 owns the first shared saga path.

V13 is accepted. The routing migration order is XY-1356/V14 complete policy and candidate-set
authority, XY-1358/V15 causal experiment authority, XY-1359/V16 atomic routing decisions, then
XY-1360/V17 continuation authority after source inspection proved durable atomic fallback state was
required. XY-1361 composes these boundaries only with production dispatch disabled. The
existing ManagedRun barrier, submitted-turn receipt, and repository/worktree/Git/artifact
reconciliation authorities retain ambiguous-effect ownership; routing never reclassifies or
replays those effects.

V16 is the sole routing-decision authority. It adds a pure routing kernel and one
operation-specific PostgreSQL exact command. The command selects and locks the immutable V14
snapshot and its current policy, run, account, compatibility, capability, blocker, and quota
sources; callers provide no candidate array or evidence. It persists one inert `selected`,
`waiting_usage`, or `no_route` row together with complete member, quota, capability, blocker, and
normalized exact-depletion references in the same transaction. Five-hour and seven-day facts,
raw timestamp text, source identity, exact microsecond precision, and evidence revision remain
separate. Missing or inexact provenance cannot establish eligibility. The adapter strictly reads
the database result back through the pure kernel. The disabled XY-1361 runtime orchestration may
invoke exactly one V16 command per request, but no daemon, application, protocol, scheduler, Codex,
credential, or UI composition root constructs that orchestration. Digest regeneration plus
executable validation remain deferred to the integrated freeze.

V17 consumes only one persisted selected V16 decision identity plus its expected ManagedRun
revision. PostgreSQL derives the selected account, V14 snapshot, source RuntimeSession,
Conversation, RoleProfile snapshot, and evidence universe; callers cannot provide candidates,
policy, exclusions, compatibility, or selection. One qualifying same-thread path requires exactly
one canonical V15 experiment lineage whose bound thread equals the source RuntimeSession thread,
a fresh positive exact `thread_read_item`, exact selected account/revision/build/RoleProfile facts,
and supported initialize/account-read/thread-read/paginated-history evidence from the exact V14
profile. Unknown, stale, negative, mismatched, incomplete, or ambiguous evidence selects the
fallback path rather than inferring compatibility.

The fallback path validates the complete deterministic Context-Pack encoding and manifest, stages
any content-addressed bytes under the existing blob coordination, then inserts its Context Pack,
selected-account snapshot, starting RuntimeSession, continuation plan, audit rows, outbox rows, and
exact receipt in one PostgreSQL transaction. A unique decision link makes both paths mutually
exclusive and exactly once across keys and replay. The plan snapshots the accepted ManagedRun
effect barrier and submitted-turn receipt count, while `replay_permitted` and `dispatch_enabled`
are structurally false. No ManagedRun identity or Conversation identity changes, no turn is
submitted or replayed. The disabled XY-1361 runtime orchestration consumes one exact V17 plan only
for a selected V16 decision; no daemon, application, protocol, scheduler, credential, Codex, or UI
composition root constructs it. Schema and configured-authority digest regeneration and all
executable acceptance remain deferred to the single integrated post-freeze gate, so ordinary
runtime readiness continues to fail closed on this moving-core tree.

V18 consumes only the exact persisted V16 `waiting_usage` decision identity and expected
ManagedRun revision. PostgreSQL derives the exact earliest-ready instant, policy lineage, run
lineage, and database clock; the caller supplies no timestamp, candidate, quota evidence,
eligibility, exclusion, account, or replacement decision. One append-only transition relation is
the lifecycle, domain-operation result, historical-readback, and cross-key replay authority. Every
accepted registration, claim, reclaim, fire, cancellation, or supersession operation has one
globally unique operation identity, canonical request, exact predecessor revision/tip, complete
resultant state, immutable effect/response bytes, and transaction-bound activity/outbox identities.
The mutable wake head is only a due-order index and current-tip fence; deferred equality and chain
checks require it to point to the exact newly appended transition. No command success or historical
readback is constructed from the head.

Unique registration-decision and run-revision links make registration converge to one durable
wake, while a different operation identity targeting an existing decision rejects instead of
aliasing it. V9 exact receipts replay byte-identically by protocol key; the same domain operation
under a new key can return only its immutable transition result after canonical request equality.
Due acquisition orders independent waits by exact earliest-ready instant, registration time, and
wake identity and never pools account quotas. A fixed sixty-second database-authored lease is
recorded on a claim or reclaim transition after global scheduler serialization. Lease expiry and
restart append a new reclaim transition rather than rewriting history. Registration, claim, fire,
and cancellation retain V16's hierarchy/run lock order, so replacement decisions cannot cross a
stale-lineage check.

Pending or leased heads advance to terminal immutable transitions when explicit cancellation,
ManagedRun revision/lifecycle/wait reason, divergence, policy revision, V16 decision kind, or
ambiguous replacement lineage is stale. A valid leased wake fires exactly once into one immutable
transition containing one `routing_resolution_request_id` whose only authority is fresh routing resolution;
`fresh_routing_resolution_only=true`, `prior_decision_reusable=false`, and
`production_enabled=false` are structural. The fired record carries no old universe or evidence,
and no runtime, protocol, daemon, CLI, scheduler composition root, Codex adapter, credential owner,
or UI imports V18. Executable timing, crash, replay, concurrency, restart, ACL, hostile-input, and
isolation acceptance remains deferred to the single integrated post-freeze gate.

V19 is an acceptance-enabling authority repair, not a scheduler feature. V18 and its checksum are
immutable history. Four new command-complete `SECURITY INVOKER` internals retain the exact V18
register, claim/reclaim, fire, and cancel transaction paths and add one nullable authority instant.
Each unchanged public `SECURITY DEFINER` command calls its schema-qualified internal with typed
`NULL`, so production still samples `clock_timestamp()` only at the original post-lock point; the
existing Rust/domain/adapter signatures and 51-function runtime execute allowlist do not change.
Only the migration owner can execute an internal with explicit time. PUBLIC and runtime execution
are revoked, the internals have no overloads or defaults, and startup closes their exact source,
metadata, ACL, dependencies, ownership, and role reachability with the canonical function and
configured-authority inventories.

An explicit instant must be finite and exactly representable in the nonnegative Unix-microsecond
domain, at most `253402300739999999`, leaving the literal 60-second lease inside the canonical
application ceiling. Registration cannot precede either the locked V16 decision time or locked
ManagedRun update time; every later explicit transition cannot precede the locked head timestamp.
These checks do not change the typed-`NULL` production path. The repair is forward-only: rollback
means restoring a pre-V19 cluster where the four internals are absent, never editing or reversing
V18. Schema and configured-authority digest regeneration remains deferred to the refrozen unified
acceptance boundary.

No production crate or application imports or constructs a V15 experiment execution root. The
mechanism-neutral core contract, PostgreSQL command adapter, and pure Codex typed-fact adapter are
the complete V15 source boundary. XY-1361 now owns the only disabled runtime orchestration over
V16 and V17: it makes one exact V16 decision per request, consumes one exact V17 plan only for
`selected`, and exposes only inert handoff facts for `waiting_usage`. It does not import V18 or
provide scheduler registration, claiming, firing, cancellation, supersession, credentials, Codex
mutation, dispatch, replay, or production enablement. The V1 protocol vocabulary remains unchanged
and exposes none of these authority commands. Production dispatch remains disabled until the
separate enablement gate.
The production runtime composes the already accepted repository owners exactly once during daemon bootstrap.
When PostgreSQL is available, it opens the pinned executor, constructs the repository saga over the
same `PostgresStore`, and performs bounded readback-only restart reconciliation before the protocol
listener can serve. Executor-open or restart-reconciliation failure leaves the repository runtime
and product-state composition unavailable and projects `ServerRepositories` unavailable in doctor;
the typed bootstrap readiness distinguishes executor, reconciliation, and residual-backlog failure.
Startup observes at most 256 eligible operations plus one residual probe and refuses repository
readiness if any eligible work remains. It does not create a second store, dispatch path, retry, or
fallback. Foreground admission, allocation, Register, WorktreeReady, and Commit enter through this
same retained runtime composition. The protocol still exposes no managed-repository mutation route
in this gate.

GitHub pull-request and check effects are a separate sealed provider boundary in
`crates/decodex-runtime/src/github_effects.rs`. It requires explicit provider/repository/revision
authority, complete pagination, durable markers, and positive readback, but XY-1353 does not invent
a live credentialed provider or persistence owner. The later Reviewer/landing owner must connect
that boundary to PostgreSQL effect lineage and an explicitly authorized provider; until then there
is no live GitHub mutation route.

Legacy `command_receipts` retain the receipt-first fenced-claim protocol only for unrelated blob,
filesystem, external, or long-running sagas whose point of no return cannot fit in one PostgreSQL
transaction; managed-repository operations do not use this protocol. Such a flow commits an
immutable pending receipt before effects; a fenced claim then
applies the expected revision, appends activity, enqueues outbox, stores exact response bytes, and
completes transaction B. Durable exact history replay retains its immutable version and referenced
blob while the legacy receipt exists. Outbox claims are bounded and
fenced by a token rotated on every claim or reclaim. Any effect that may have begun must be
reconciled through a meaningful receipt and authoritative readback after claim expiry or
restart. Lease, retry, and retention durations are exact positive whole milliseconds capped
at 365 days; stored lease functions enforce the same fixed-millisecond boundary, lease rows
cannot exceed it relative to their update time, and in-flight outbox leases carry a persisted
claim-or-renewal timestamp anchor. Delivered outbox retention is finite, positive,
whole-millisecond, chronological, and capped at 365 days. Delivered rows are terminal and
immutable until retention pruning is due; no other outbox state is deletable, and table
truncation is forbidden. Direct SQL therefore cannot delete and recreate a completed external
effect as replayable work.
Operation-time triggers reject caller-shifted anchors and
deadlines beyond the same 365-day horizon; relative-duration `CHECK` constraints remain
wall-clock independent. Quota mutation responses and command receipts use already validated exact
UTC Unix-microsecond values, persisted losslessly with no rounding or truncation, rather than caller
timestamp text. Account and quota-window rows are
inert observations with recursive credential-material rejection across normalized keys and
recognizable secret-bearing value encodings. PostgreSQL explicitly normalizes Rust's full
Unicode `White_Space` set, applies an explicit ASCII case fold, and evaluates the remaining
case-sensitive regular expressions under the built-in `C` collation. The integration gate
repeats credential vectors in a Turkish ICU database so database-default case rules cannot
weaken the direct-SQL boundary; this crate exposes no
eligibility, account selection, fallback, wake scheduling, or credential storage.

Provider ingress must additionally retain the exact raw timestamp representation until exact UTC
Unix-microsecond construction succeeds. V14 through V16 do not assume a provider precision and
fail closed on any value that would require rounding or truncation. Natural characterization and
retained-title Desktop discovery remain post-freeze evidence owned by XY-1357 and XY-1363,
respectively; neither is a runtime authority path.

## Owned vNext paths, configuration, blobs, and cache

`crates/decodex-core/src/paths.rs` owns one absolute, lexically normalized Decodex root
and the typed private-filesystem contract. On Unix, `path_unix.rs` implements that
contract by opening every component relative to an already validated directory
descriptor; reads, listing, removal, temporary-file publication, and synchronization
therefore cannot be redirected by an ancestor rename or symlink swap. `identity.rs`,
`blob.rs`, and `cache.rs` own their independent persistence/integrity/bounding policies;
`storage.rs` owns only their shared redacted error contract.
The platform default is `~/.decodex`; configured roots are bounded to 4 KiB and roots
below any `.codex` component are rejected before I/O. Existing root ancestors, the root,
and every owned descendant reject
symlinks and unexpected file kinds. On Unix, every owned directory and file must belong
to the effective OS user; directories must be private mode 0700 and files must deny
group/other and executable access. Owned writes use same-directory
private temporary files, file and directory synchronization, and atomic rename or
create-only hard-link publication. Root layout is fixed:

- `config.toml`: at most 64 KiB, UTF-8 TOML, owner-readable and non-executable with
  no group/other access (normally mode 0600; mode 0400 is also accepted);
- `logs/`: Decodex log ownership only; no logging runtime is added by XY-1306;
- `blobs/sha256/<prefix>/<digest>`: at most 64 MiB per in-memory write, atomically
  published and fully SHA-256-verified on existing writes and reads;
- `cache/`: disposable hashed entries bounded simultaneously by configured per-entry,
  aggregate-byte, and entry-count caps under hard ceilings; recovery removes only exact
  private `.tmp-<32 lowercase hex>` artifacts left by interrupted atomic writes; and
- `server/identity`: one canonical RFC 9562 UUID version 4 generated from OS randomness,
  persisted create-only, and stable across concurrent initialization.

`crates/decodex-core/src/config.rs` denies unknown fields and discards parser details and
input excerpts from typed errors. Debug implementations redact operator-provided profile,
host, repository, database, role, and credential-reference strings. Profiles are a closed
`local`/`remote` enum: local addresses must be loopback; remote profiles carry only a
bounded host, port, and required expected server identity. Repository roots exist only in
`ServerHostConfig` as absolute, normalized, at-most-4-KiB `ServerRepositoryPath` values,
so a remote profile has no client-local repository-path field. PostgreSQL configuration
is one explicit bounded Unix-socket directory/port/database, an expected server peer UID, plus
distinct migration/runtime users and optional, distinct credential environment-variable references.
`decodex.example.toml` is the redacted canonical shape.
XY-1307 consumes that data at the runtime composition boundary; core itself still opens
no database. No CLI, remote listener/security, or credential-vault implementation is part
of the core foundation.

## Active Codex adapter foundation

`crates/decodex-codex/` structurally extracts request/notification methods from generated
schema, validates native-collaboration object variants, required fields, field types, and
referenced enums from `ThreadReadResponse`, and compares canonical JSON digests with the
accepted XY-1262 receipt before spawning an app server.
Marker presence is only schema evidence: live method outcomes become explicit `supported`,
`unsupported`, `unavailable`, or `degraded` states. Every successful probe must enter its
profile into the exact-build cache, which rejects conflicting replacement and has no
nearest-build or stale fallback.

Each `SupervisedProcess` owns one immutable shared-home account authority. Credential
environment variables are removed from the child, initialize must report the normal
`$HOME/.codex`, no credential-switch operation exists, and read-only `account/read` must
establish an exact redacted, zeroizing in-memory account identity before a successful probe. On Unix the child starts in a new session;
bounded shutdown checks the entire process group independently of the leader and escalates
from termination to kill. Raw JSON-RPC, stderr, account details, and free-form model deltas
stay inside the adapter. Callers receive typed probe results, stable errors, correlation-
only message events, and run-local collaboration actors identified by validated UUIDs or
digests rather than optional nickname/role fields. Build identity is an exact opaque
fingerprint; statuses, activity kinds, tools, capability reasons, and read-only methods
are closed enums, so protocol text is not exported through debug or serialization paths.

The production command opens the canonical `codex` executable and first rejects interpreter-driven
files. On macOS, only current native 64-bit thin Mach-O images and native 32/64-bit universal Mach-O
containers cross this boundary. On Linux, only native ELF images cross it, and account-bound launch
additionally requires executable sealed-memfd support (`MFD_EXEC`, `F_SEAL_EXEC`, and the write,
grow, shrink, and seal seals) plus procfs descriptor execution. A host missing any required Linux
primitive returns executable-unavailable during command construction, before vault projection.
Shebang wrappers and other formats therefore fail before vault projection on both platforms.
The platform loader remains responsible for validating the complete image and selecting an
architecture slice or ELF interpreter; dynamic-loader and shared-library integrity remain part of
the host OS trust boundary rather than the build digest.

The runtime copies the accepted source descriptor into a bounded protected object and hashes that
object for the opaque build identity. On macOS this is a private fsynced mode-0500 file protected by
`UF_IMMUTABLE`. On Linux it is an fsynced mode-0500 memfd whose contents, size, executable mode, and
seal set are irreversibly sealed; the runtime verifies every required seal and that
`/proc/self/fd/<owned-fd>` resolves to the same object. Every version, schema, and app-server spawn
executes that exact protected object. The Linux descriptor is close-on-exec: it remains open while
the kernel resolves the native ELF image and is closed atomically on successful exec. Original-path
identity and digest checks detect pre-launch drift, but are not the check-to-exec security
primitive. A source replacement after the final verification cannot alter the protected object that
executes or receives credentials. Failure to create, seal, resolve, or execute the object fails
closed. This protection assumes the daemon process and its uid are not already compromised. Only
tests can inject a fake executable. Preflight output/files, generated-schema file count/per-file/aggregate
bytes/depth, inbound and outbound app-server frames, the stdout queue, collaboration
receiver count, and thread-list/search results are mechanically bounded; schema traversal
rejects symlinks and special files. Both preflight commands use bounded process-group
supervision and descendant cleanup. Failed bounded cleanup transfers the still-owned child, process
group, and lifetime guard to the hard-capped fair quarantine rather than relinquishing process
authority. Transfer is bounded; background retries are isolated per round and may intentionally
retain capacity indefinitely when group death cannot be confirmed.

The probe sends `initialize`, `initialized`, read-only `account/read`, bounded
`thread/list(useStateDbOnly=true)`, exact-ID `thread/read(includeTurns=false)` when a listed
thread exists, and `thread/search` with a fixed nonmatching term and bounded result count.
The latter two calls establish method availability only; they do not claim global title
discovery. Account identity is pseudonymized and re-attested after every authority read;
an identity change discards the in-progress negotiation, and restart retains the expected
identity for re-attestation. Archive, paginated persisted history, and native collaboration
remain explicitly `not_probed` while their side-effect/event gates are closed. Paginated
history schema evidence comes from the structural `ThreadStartParams.historyMode` /
`ThreadHistoryMode` contract, never from a list cursor.
`DispatchGate` has no enabled state and denies thread start/resume, turn start/steer/
interrupt, and approval responses under XY-1304. The composition root therefore still
reports conversation execution unavailable even though schema validation, read-only
probing, normalization, and supervision are implemented.

## Frozen v0.2 runtime provenance

Everything below this heading maps the preserved `apps/decodex/` source. That package
is excluded from the active Cargo workspace and is not a runtime, fallback, compatibility
facade, or implementation authority for vNext.

`apps/decodex/src/main.rs` calls `decodex::run()`. `run()` does four things (`apps/decodex/src/lib.rs`):

1. Install `color_eyre` error reporting.
2. Initialize daily rolling tracing files in `~/.codex/decodex/logs`.
3. Install a panic hook that aborts after the default panic output.
4. Parse and run the Clap CLI.

The crate has a compile-time Unix-only guard: macOS and Linux are supported; Windows is rejected (`apps/decodex/src/lib.rs`).

`apps/decodex/src/cli.rs` owns the command surface. Current top-level commands include:

- `run`, `serve`, `status`, `project`, `lane`, `diagnose`, `evidence`, `recover`, `intake`, `mcp`, `probe`, `verify`
- `commit`, `land`, `git-hook` for Decodex-owned Git lifecycle policy
- `account`, `app`, `archive-linear`, `maintenance`
- hidden `_attempt` for daemon-planned child attempts

## Runtime state layout

All local runtime state is under `~/.codex/decodex` (`apps/decodex/src/runtime/paths.rs`):

- `config.toml`: global operator config.
- `accounts.jsonl`: shared ChatGPT/Codex account pool.
- `projects/`: registered project contract directories.
- `logs/`: Decodex tracing logs.
- `agent-evidence/`: derived repair-agent evidence views.
- `runtime.sqlite3`: single-machine runtime database.

`StateStore` opens the runtime DB and bootstraps schema with WAL enabled (`apps/decodex/src/state/sqlite_store/schema.rs`). Base tables include projects, leases, run attempts, protocol events and summaries, run activity summaries, worktrees, and Linear execution events. Bootstrap then adds worktree, review, evidence artifact, run-control, connector backoff, private execution event, Decision Contract, autonomy, Execution Program, Program Intake, loop guardrail, and migration schemas.

## Project contracts

A project is not discovered from a checkout. It is explicitly registered from a project directory containing `project.toml` and `WORKFLOW.md` (`apps/decodex/src/config/service.rs`, `apps/decodex/src/cli/control_commands/project.rs`). `project.toml` fields are parsed by `ServiceConfigDocument` (`apps/decodex/src/config/document.rs`):

- `service_id`
- `[tracker]`
- `[github]`
- optional `[codex]`
- optional `[autonomy]`
- optional `[privacy_classifier]`
- `[paths]`

At the v0.2 freeze, `decodex.example.toml` was the safe project-config model. The current
checked-in file now owns the vNext global config shape above and is not an input or
compatibility template for the frozen package.

## One-shot run flow

`decodex run` is implemented by `apps/decodex/src/cli/control_commands/run.rs` and `apps/decodex/src/orchestrator/entrypoints/run.rs`.

The high-level flow is:

1. Open the global runtime store.
2. Resolve a project config from `--config`, current checkout registry mapping, or the registered project table.
3. Register or refresh the project config in runtime state.
4. Load the project `WORKFLOW.md`.
5. Respect stored tracker connector backoff.
6. Optionally explain the queue for `--dry-run --explain`.
7. Call `run_configured_cycle`.

`run_configured_cycle` loads `ServiceConfig`, workflow, and a Linear client. If an issue id is supplied, it runs that target issue with inferred or explicit dispatch mode; otherwise it runs project selection (`apps/decodex/src/orchestrator/run_cycle.rs`). Preparation validates workflow read-first files, plans worktree state, resolves run identity, acquires leases, and materializes the lane through the run-cycle modules.

Recent source adds a baseline guard before ordinary, Program, and retry dispatch. `ensure_clean_baseline_before_dispatch` checks workflow canonicalization commands, records private events, serializes normalization with `.decodex-baseline-normalization.lock`, may create/land a baseline normalization PR, and blocks if canonicalization still rewrites tracked files (`apps/decodex/src/orchestrator/baseline_guard.rs`). This came from recent commits titled "Guard baseline canonicalization before dispatch" and should be considered part of current dispatch safety.

## Long-running control plane

`decodex serve` calls `orchestrator::run_control_plane` through `apps/decodex/src/cli/control_commands/serve.rs`. The operator listener default is `127.0.0.1:8192` in README examples and OpenWiki operator notes. `--dev` is hidden and is only for isolated endpoint testing; it does not represent normal scheduling.

Each daemon tick (`apps/decodex/src/orchestrator/daemon.rs`):

- reconciles active child process state and retry queue entries
- recovers and reconciles idle project state when no active children exist
- reconciles post-review orchestration
- reconciles terminal thread archive backlog
- spawns due child attempts until no more can start

`openwiki/workflows/runtime-operator-workflows.md` records the current cadence: operator snapshots publish every 15 seconds, and Linear-backed queue/status scans run at most every 5 minutes per project unless `POST /api/linear-scan` requests a scan.

## Frozen app-server execution

The excluded v0.2 `apps/decodex/src/agent/app_server/run.rs` owned direct live
`codex app-server` execution. It is provenance only and must not be used by vNext. One
legacy attempt:

1. Records run attempt status as `starting`.
2. Writes activity markers when configured.
3. Publishes a run-control channel for lane control.
4. Spawns `codex app-server` through the JSON-RPC client.
5. Initializes the client and records user-agent/capability evidence.
6. Runs capability preflight and optional `command/exec` health check.
7. Logs into a selected Codex account when account-pool routing is configured.
8. Starts or resumes a thread session.
9. Records `running`, executes the turn loop, then records `succeeded`.
10. Retires the run-control channel as completed or failed.

`openwiki/specs/contracts-and-data.md` summarizes protocol requirements: Decodex uses `stdio://`, expects generated-schema compatibility, requires phase-goal methods, exposes issue-scoped dynamic tools, and treats `decodex probe stdio://` with `PROBE_OK` as a live compatibility check.

## Tracker bridge and completion

The issue-scoped tracker bridge in `apps/decodex/src/agent/tracker_tool_bridge.rs` binds the agent to one leased issue. It exposes dynamic tool names such as `issue_transition`, `issue_comment`, `issue_label_add`, `issue_progress_checkpoint`, `issue_review_checkpoint`, `issue_review_handoff`, `issue_review_repair_complete`, `issue_closeout_complete`, and `issue_terminal_finalize`.

The important architecture boundary is not the tool list itself; it is who owns what:

- The agent may perform bounded issue-scoped tracker writes through dynamic tools.
- The runtime still owns leases, worktrees, retries, recovery, crash fallback, post-review lifecycle, and cleanup.
- Private evidence goes into runtime SQLite before any public Linear projection when a tool has both private and public effects.
- Terminal completion must be explicit; the runtime should not guess whether a lane meant review handoff, manual attention, repair completion, or closeout.

## Operator HTTP and dashboard

`apps/decodex/src/orchestrator/operator_http.rs` owns the local HTTP endpoint, dashboard support constants, API routes, and WebSocket/control traffic. Routes are parsed in `apps/decodex/src/orchestrator/operator_http/routes.rs` and dispatched by `apps/decodex/src/orchestrator/operator_http/server.rs`: `/livez`, `/api/operator-snapshot`, `/dashboard/control`, `/api/accounts`, `/api/linear-scan`, `/api/lane/inspect`, `/api/lane/interrupt`, and `/api/lane/steer`/`/api/lane-steer`. `apps/decodex/src/orchestrator/operator_http/assets.rs` defines the dashboard/client limits, HTTP read timeout, and volatile run-activity fingerprint fields.

The published dashboard snapshot is a cached JSON serialization of `OperatorStatusSnapshot` protected by the `OperatorStateEndpoint` mutex; `publish_snapshot` is the only writer and broadcasts a `snapshot` WebSocket event after updating it (`apps/decodex/src/orchestrator/types/operator_endpoint.rs`). `/api/operator-snapshot` and the dashboard WebSocket read from that cache, inject live global account-control state, and add presentation fields (`apps/decodex/src/orchestrator/operator_http/snapshot.rs`). Runtime lifecycle state still belongs to SQLite, leases, worktrees, tracker ledgers, and daemon/recovery code; HTTP and WebSocket paths are readback/control projections, not new ownership stores.

Dashboard streaming has two readback paths. `snapshot` events follow control-plane publication, normally once per 15-second tick; `runActivity` events are a lighter 1-second stream that rebuilds current-lane projections without live external observers, strips volatile timing fields from its fingerprint, suppresses duplicate broadcasts, and can be filtered by project, issue, or run subscription (`apps/decodex/src/orchestrator/constants.rs`, `apps/decodex/src/orchestrator/operator_http/dashboard/run_activity.rs`, `apps/decodex/src/orchestrator/operator_http/dashboard/subscription.rs`). Dashboard control messages are intentionally narrow: focus/subscription changes are session-local, acknowledgements are session-local, and account selection only changes the global Codex account selector through the accounts module (`apps/decodex/src/orchestrator/operator_http/dashboard/control_actions.rs`).

Account HTTP APIs delegate to account-domain commands: list with optional usage refresh, select, clear, logout, import, use, and reroll name (`apps/decodex/src/orchestrator/operator_http/api/account.rs`). Lane inspect, interrupt, and steer delegate to `lane_control` with registered-project resolution and keep audit/control effects in runtime state (`apps/decodex/src/orchestrator/operator_http/api/lane.rs`). Private execution evidence is read back through `decodex evidence`, where `build_private_evidence_readback` loads local SQLite private events, summarizes payloads by default, and emits stable `private-evidence:<project>/<issue>/<run>/<attempt>` references (`apps/decodex/src/orchestrator/agent_evidence/private_readback.rs`); dashboard/operator status may show the reference and read command, but raw private payloads are not a public dashboard contract.

Linear-facing updates stay sparse. The progress checkpoint tool first records the full execution-state checkpoint as private runtime evidence, then publishes only a low-frequency public Linear projection when the public lifecycle signal/idempotency key changes (`apps/decodex/src/agent/tracker_tool_bridge/tools/progress_checkpoint/handler.rs`, `apps/decodex/src/agent/tracker_tool_bridge/tools/progress_checkpoint/projection.rs`). Control-plane Linear scans run at most every 5 minutes per project unless `/api/linear-scan` queues an all-project or project-scoped scan for the next tick; that request still waits behind active tracker connector backoff (`apps/decodex/src/orchestrator/entrypoints_control_plane.rs`, `apps/decodex/src/orchestrator/entrypoints_control_plane/project_tick.rs`, `apps/decodex/src/orchestrator/operator_http/api/linear_scan.rs`). Rate-limit and timeout detection are modeled as connector backoff warnings (`tracker_rate_limited`, `tracker_transient_timeout`), persisted in runtime state, included in snapshots, and used to skip external observer reads until retry is safe (`apps/decodex/src/orchestrator/entrypoints_tracker_backoff.rs`, `apps/decodex/src/orchestrator/status/snapshot/observers/backoff.rs`).

Current non-goals are important boundaries: the operator HTTP server is loopback-local by default, not a durable workflow engine; dashboard focus/ack messages are not lifecycle events; account controls affect future account selection but do not rewrite existing lane authority; `/api/linear-scan` requests observation rather than dispatch; and lane steering/interrupt APIs must continue to honor lane-control preconditions, leases, private audit evidence, and tracker/public-text privacy boundaries.

## MCP gateway

`apps/decodex/src/mcp.rs` serves MCP over stdio or Streamable HTTP:

- Stdio defaults to `admin` capability profile.
- Streamable HTTP defaults to `observe`, binds to `127.0.0.1:8193`, serves `POST /mcp`, validates origins, manages `Mcp-Session-Id`, and requires bearer auth for non-loopback or profiles above observe (`apps/decodex/src/mcp.rs`, `openwiki/workflows/runtime-operator-workflows.md`).
- Tool profiles are `observe`, `plan`, `operate`, and `admin`.
- Tools include `decodex_observe`, `decodex_plan`, goal/autonomy planning tools, `decodex_lane_control`, and `decodex_project_control` (`apps/decodex/src/mcp.rs`).

MCP is a typed facade over existing runtime and operator controls. It is not a bypass around Decision Contract acceptance, lane-control preconditions, tracker boundaries, review policy, or project enablement. The design rationale for keeping MCP typed and skills slim lives in [Design rationale](../decisions/design-rationale.md); the current remote-control drift audit lives in [Drift audits](../evidence/drift-audits.md).

## Change guidance

- CLI changes: start in `apps/decodex/src/cli.rs` and the owning submodule under `apps/decodex/src/cli/`; add parser tests under `apps/decodex/src/cli/tests/`.
- Runtime scheduling changes: start in `apps/decodex/src/orchestrator/run_cycle.rs`, `apps/decodex/src/orchestrator/daemon.rs`, and the lifecycle-specific orchestrator submodule; expect dense tests under `apps/decodex/src/orchestrator/tests/`.
- State changes: start in `apps/decodex/src/state/sqlite_store/schema.rs`, migrations, row parsers, and `StateStore`; protect replay/idempotency with state tests.
- vNext app-server foundation changes: start in `crates/decodex-codex/`; run its schema,
  fake-process, supervision, redaction, and dispatch-guard tests. The excluded
  `apps/decodex/src/agent/app_server/` is provenance only.
- Operator/MCP changes: update HTTP/MCP tests and check public/private projection boundaries before exposing new fields.

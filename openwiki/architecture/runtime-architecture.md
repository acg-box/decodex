# Runtime Architecture

Status: accepted vNext target architecture. Current source still contains superseded
numbered schema and startup-provisioning machinery. This document defines the replacement
target; it does not claim that current source has completed the reset.

There are no external or deployed Decodex users. Local PostgreSQL state is disposable
development state. The architecture supports an empty-target latest-schema bootstrap,
not database upgrades.

## Process topology

`decodexd` is the only product daemon and server composition root. It owns:

- the owner-only same-UID Unix WebSocket listener;
- PostgreSQL runtime connections and product-state adapters;
- Account Service and HostCredentialStore access;
- Codex app-server child processes;
- ProcessSupervisor and ProviderAttemptService;
- repository and worktree effects;
- provider observation and reconciliation; and
- bounded shutdown of every admitted task and child.

`apps/decodex-cli`, `apps/decodex-gpui`, and `apps/decodex-app` are clients. They do not
read PostgreSQL, credentials, rollout files, blob paths, repositories, or provider-private
identities directly. `crates/decodex-runtime` is the only library owner that composes the
protocol and infrastructure adapters.

The local transport endpoint is `~/.decodex/server/decodex.sock`. The server directory,
persistent lock, staging socket, and published socket keep their existing owner-only
mode, ownership, one-link, descriptor-relative publication, peer-UID, and replacement
checks. Remote, cross-UID, and unauthenticated loopback control remain disabled.

## One latest schema

`crates/decodex-postgres/schema.sql` is the only executable PostgreSQL schema owner. It is
unversioned and represents the complete latest accepted schema. It contains final
definitions directly for:

- `pgcrypto` and the Decodex namespace;
- all enum labels;
- all relations, columns, defaults, constraints, and indexes;
- all functions and exact function-local settings;
- all ordinary and constraint triggers and their final bindings;
- stable object dependencies;
- owners, grants, revokes, and default privileges; and
- the exact current `schema_fingerprint` and runtime-authority binding.

It contains no numbered step, ordered history, old-state predicate, upgrade prefix,
drain, backfill, compatibility branch, rollback branch, finalizer, or fallback. It does
not alter or drop an old Decodex object to reach the latest shape. Required final enum
values and object bodies appear in their final `CREATE` definitions.

The old numbered SQL directory, Refinery dependency and runner, schema-history relation,
version constants, migration error surface, migration-source includes/parsers, prefix and
upgrade test helpers, and executable schema in `spikes/vnext-storage` are rejected. They
must be deleted directly. No removal migration is allowed.

### Schema module boundary

The allowed `decodex-postgres` Rust `schema` module has three inseparable duties:

1. prove the clean-target precondition;
2. execute the one latest schema in one transaction; and
3. run exact post-execution catalog and configured-authority verification before commit.

The module is not allowed when it only wraps `include_str!`. There is no
`SchemaManager`, registry, version constant, generated schema pipeline, bootstrap facade,
or cutover coordinator. Authority-verifier declarations are read-only expected facts;
they cannot create, alter, repair, or become a second executable schema.

### Empty-target bootstrap

Schema bootstrap is an explicit operator operation, separate from the daemon serve path.
The implementation-owned command must:

1. accept one explicitly configured PostgreSQL 18 Unix-socket endpoint and an
   operator-pinned expected server UID;
2. resolve the schema-owner credential only for this invocation;
3. verify the socket directory and endpoint by descriptor identity and kernel peer UID
   before authentication data is sent;
4. require data checksums and an exact accepted fresh-database catalog baseline, apart
   from externally provisioned login roles; no user-created schema, relation, function,
   type, extension, or other product object may exist;
5. begin one transaction;
6. execute the complete latest schema once;
7. verify PostgreSQL major, checksums, `pgcrypto`, catalog shape, dependencies,
   ownership, ACLs, function/trigger semantics, `schema_fingerprint`, and configured
   runtime authority; and
8. commit only when every verification succeeds.

The operation fails closed on a nonempty target. The schema it creates makes a second
invocation fail. There is no idempotent "ensure schema" mode.

The exact command name is implementation-owned and is not yet specified. Installer code
may invoke the accepted operator bootstrap for a newly created local target, but it must
not move schema creation into normal daemon startup.

### Runtime startup

Normal `decodexd` startup resolves only the runtime PostgreSQL credential. It resolves no
schema-owner credential and executes zero DDL. Its database sequence is:

1. verify the configured Unix-socket path and expected server UID;
2. connect as the configured runtime identity;
3. verify PostgreSQL 18, data checksums, and `pgcrypto`;
4. verify the exact current catalog and `schema_fingerprint`;
5. verify the complete configured-authority and semantic contract; and
6. retain the runtime pool only after all checks pass.

Startup never creates, upgrades, repairs, finalizes, or rolls back a schema. It does not
read a schema-owner password, numbered SQL, a schema-history relation, an old database,
or a cutover receipt. Any mismatch maps to typed unavailable product state. Doctor/status
revalidates the same read-only current-authority boundary and never runs DDL or repins an
endpoint.

### Current catalog and authority verification

The verifier keeps the existing PostgreSQL 18 safety depth. It checks the exact closed
inventory and semantics of schemas, relations, columns, defaults, enum labels,
constraints, indexes, functions, triggers, rules, policies, RLS state, sequences, and
stable catalog dependencies. Foreign keys are checked when a Decodex relation is either
the child or parent. Extension authority follows dependency membership, not only schema
placement.

For each function, verification covers identity, overload set, owner, language,
`SECURITY DEFINER`/invoker status, volatility, parallel safety, configuration, source
body, ACL, and dependencies. For each trigger it covers name, relation, timing, events,
constraint semantics, enabled state, function binding, and dependencies. No unexpected
user trigger, rule, policy, RLS mode, overload, or expression dependency may create an
indirect runtime path.

Configured-authority verification starts at the runtime login and follows every inherited,
NOINHERIT, membership-admin, and `SET ROLE` path. It rejects:

- superuser, `BYPASSRLS`, role administration, or ownership of a Decodex object;
- database, schema, table, function, trigger, rule, policy, sequence, or extension DDL;
- `TRUNCATE`, grant options, unsafe default privileges, or retention bypass;
- `session_replication_role` or equivalent trigger-bypass authority;
- relation DML or helper execution outside the exact adapter contract;
- sequence authority beyond exact required `USAGE`;
- PUBLIC execution or relation authority outside the closed contract; and
- hostile `search_path`, same-signature overload, external cascade, or extension-member
  paths that could run with stronger authority.

All database functions use exact safe function-local search paths. Trigger-only helpers
are not directly executable by runtime or PUBLIC. The runtime role cannot mutate current
catalog identity or `schema_fingerprint`.

The verifier has no schema-history, version-prefix, checksum-ledger, or upgrade predicate.
It does not parse a sequence of SQL sources. Historical relation/function/trigger counts
are not authority. The exact current candidate establishes its own reviewed closed
inventory.

## Runtime composition and readiness

One `decodexd` and one owner-only same-UID endpoint remain authoritative. A Quick Task or
ManagedRepository startup failure does not terminate the daemon when the transport and
control plane can start. Diagnostics, account recovery, and available PostgreSQL-backed
reads remain usable.

Composition records these independent startup projections:

| Surface | Startup projection | Owner boundary |
| --- | --- | --- |
| `ProductStore` | `Available(PostgresStore)` or `Unavailable(ProductStateReason)` | Exact runtime PostgreSQL connection and current-authority verification only. |
| Quick Task | `Ready(QuickTaskRuntime)` or `Unavailable(QuickTaskUnavailableReason)` | Stateless Quick Task sequencing after all fallible dependencies are validated. |
| ManagedRepository | `Ready`, `Disabled`, or `Unavailable(ManagedRepositoryUnavailableReason)` | Optional repository path, Git, executor, and reconciliation assembly. |

`ProductStore` never contains Quick Task or repository readiness. A repository path,
Git executor, reconciliation, or configuration failure cannot invalidate a verified
PostgreSQL store. A Quick Task dependency failure cannot invalidate PostgreSQL or
ManagedRepository.

All fallible dependency owners finish their I/O, attestation, and startup work before
Quick Task construction. `QuickTaskRuntime::new` or its final equivalent is infallible and
performs no I/O. Service composition stores one immutable ready or unavailable projection
for that daemon process. It does not poll, mutate, promote, or recover that projection.
Every Quick Task command repeats the current Account Service, routing,
ProcessGeneration, ProviderAttempt, app-server, and path fences before it can cause an
effect.

This structure has no capability-manager module. The startup projections own no durable
state, receipt, task, channel, retry, or lifecycle. They only make the result of service
assembly explicit. A deletion test must continue to hold: removing the Quick Task owner
would spread sequencing and its closed error projection into callers; removing a wrapper
that only forwards these values must remove no required behavior and is not allowed as a
new module.

Core configuration owns transport and PostgreSQL runtime configuration. It does not
require a static repository path map. PostgreSQL owns repository identity, admission, and
persisted path policy. If one concrete accepted host-only repository policy needs local
configuration, that configuration has its own parser and validator. Missing or invalid
repository configuration maps only to ManagedRepository `Disabled` or `Unavailable`; it
cannot prevent core configuration parsing, endpoint binding, PostgreSQL verification, or
Quick Task composition.

Protocol and doctor return the three readiness projections independently. Quick Task
execute, start, and resume return `QuickTaskUnavailable` with a closed redacted reason
when Quick Task is unavailable. Persisted list and get operations return
`ProductStateUnavailable` when PostgreSQL is unavailable. No optional setter, `.ok()`
conversion, or omitted field can turn a startup error into silent feature absence.
`AcceptanceUnknown` and recovery-required responses remain effect and recovery results;
they are not startup readiness.

## State ownership

| State or surface | Authority |
| --- | --- |
| Projects, Agents, policies, Programs, Objectives, WorkItems, ManagedRuns, Automations, profiles, context, messages, mappings, and UI-visible activity | PostgreSQL domain relations with optimistic revisions, leases, append-only activity, and transactional outbox |
| Conversations, Turns, RuntimeSessions, history, Context Packs, and routing | PostgreSQL current domain authority |
| Account product state | PostgreSQL Account Registry |
| Credentials | one versioned HostCredentialStore |
| Provider process lifecycle | ProcessSupervisor plus ProcessGeneration records |
| External Codex turn effects | ProviderAttemptService plus ProviderAttempt records |
| Repository operation authority and evidence | PostgreSQL managed-repository state |
| Repository bytes and worktrees | Git and filesystem on the daemon host |
| PR/check/merge readback | GitHub |
| Large output/evidence bytes | local content-addressed blob store with PostgreSQL metadata |
| Codex rollout and thread visibility | shared normal `~/.codex` |
| GPUI cache | bounded disposable local cache only |

PostgreSQL is not event sourced. `schema_fingerprint`, exact-command receipts,
account-operation records, ProcessGeneration, ProviderAttempt, activity, outbox, and
repository-effect records are current domain integrity. They do not record schema
history or grant schema bootstrap authority.

## Exact commands

Pure database mutations use operation-specific, command-complete database functions.
PostgreSQL constructs the complete typed request envelope and authoritative response.
Runtime supplies a protocol-scoped idempotency key and typed operation inputs. It does not
supply an authoritative request hash, selected row, generated identity, timestamp,
snapshot, activity identity, or outbox identity.

An exact receipt can be executing only inside its top-level transaction. Commit-time
completeness rejects an incomplete receipt. Completed success and stable rejection are
immutable and replay exact stored response bytes. Mechanical database failure rolls back
instead of becoming a stable domain rejection. The runtime role has no exact-receipt
relation privilege or private-helper authority.

These records protect command idempotency and atomicity. They are not schema bootstrap,
schema upgrade, or schema-history records.

## Candidate-5 Quick Task

Candidate 5 is the accepted target for one ordinary multi-turn Quick Task. It is not a
ManagedRun and has no WorkItem, reviewer, PR, harness, or Goal. Current source must be
aligned to this target while preserving current-main account observation behavior.

### Owner order

The exact initial order is:

```text
Conversation create
-> prospective Turn UUID intent
-> Routing Snapshot
-> Routing Decision
-> first account/profile snapshots + starting RuntimeSession + inert initial plan
-> Conversation-owned atomic Turn/history admission
-> Account Service selected-account pre-spawn fence
-> fresh ProcessGeneration
-> RuntimeSession thread fence/start/bind
-> ProviderAttempt
```

Routing Snapshot locks the complete policy member universe, canonical order, account
revisions, two separate quota facts per member, required capability facts, and blockers.
Routing Decision is the sole account selector. Account Service owns lifecycle facts and
rechecks only the selected account's current readiness, credential version/fingerprint,
provider binding, and HostCredentialStore binding immediately before spawn. It cannot
preselect, replace, fall back to, or wake another account.

Continuation Plan consumes the selected initial decision. In one transaction it creates
the selected account snapshot, copied RoleProfile snapshot, first revision-1 unfenced
`starting` RuntimeSession, inert `initial_thread` plan, exact receipt, activity, and
outbox. Failure rolls back the complete cluster.

Conversation authority then admits exactly one Turn with the prospective UUID, sequence
1, role `user`, `possible_side_effects=unknown`, status `active`, and revision 1 under the
same Conversation and new RuntimeSession. The same transaction creates exactly one
ordinal-0 completed Message plus receipt/activity/outbox. A competing key creates no
partial domain effect. No Turn row exists before this transaction.

### Closed lineage

The latest schema defines two routing lineage shapes:

- `L0`: all six RuntimeSession, account-snapshot, and profile-snapshot identity/revision
  fields are null; and
- `L6`: all six are present and the three revisions are positive.

Conversation routing permits `L0` or `L6`. ManagedRun routing permits only `L6`. All
existing foreign keys remain. One exact all-null/all-present constraint rejects partial
lineage.

`L0` requires one open exact-revision Conversation, no existing RuntimeSession or Turn,
zero sticky members, and exact equality with the complete locked routing universe.
Source fields, sticky members, closed or stale Conversation state, a source-less
ManagedRun, or missing, extra, duplicate, cross-linked, or reordered evidence fails
closed. `L6` retains the existing exact one-sticky-member rule.

There is one initial route decision. Same-key replay is read-only. A competing cross-key
initial decision loses under the Conversation lock. Initial planning is first-session
creation and cannot enter same-thread reuse, explicit successor, or Context Pack fallback.

### Thread and effect fences

Before ProcessGeneration preparation or spawn, each thread fence/start/bind, and each
ProviderAttempt prepare/authorize operation, the owner locks and verifies the exact
selected Turn as active revision 1 under the same Conversation and RuntimeSession.

ProcessGeneration and thread establishment through bind require the applicable
`starting` RuntimeSession revision. ProviderAttempt preparation and dispatch authorization
require the exact post-bind `active` revision and the exact completed thread fence and
bind receipts. A terminalization race loses before an effect.

Only `Fresh` ProcessGeneration outcome returns the non-clone spawn authority. `Replayed`,
`Rejected`, and `Unknown` return readback or refusal without spawn authority. They cannot
spawn, replace, adopt, create a successor, duplicate an attempt, or terminalize the Turn.

Conversation authority may move the active revision-1 Turn to `failed` revision 2 under
the starting session only when positive readback proves definite pre-effect refusal. The
proof must exclude any process state that may have created a child, every thread fence or
start/bind, and every prepared, authorized, or unknown ProviderAttempt. Ambiguous work
keeps the Turn active and returns `Unknown` for manual recovery.

Explicit successor is PostgreSQL-only non-dispatch evidence. It locks the exact Turn named
by the route decision before any write and requires the same Conversation/source session,
status `failed`, and revision 2. It has no product or runtime mutation surface.

### Final trigger definitions

The latest schema creates the final bodies and unchanged bindings for these seven affected
trigger functions:

| Function | Final Candidate-5 responsibility |
| --- | --- |
| `decodex.enforce_routing_completeness()` | Preserve complete existing `L6` policy/evidence rules and add only the exact zero-sticky `L0` branch. |
| `decodex.enforce_runtime_session_state()` | Permit only request fencing, exact start-response/thread binding, and last-Turn acknowledgement as the narrow nonterminal edges; reject generic `starting` to `active`. |
| `decodex.enforce_turn_state()` | Under `starting`, permit only exact first-Turn admission and positive definite pre-effect failure; preserve all existing active-session behavior. |
| `decodex.enforce_history_item_state()` | Under `starting`, permit only the admission transaction's exact ordinal-0 completed Message; preserve existing active-session behavior. |
| `decodex.enforce_provider_attempt_transition()` | Keep the accepted state algebra and add the immutable RuntimeSession thread-binding receipt identities. |
| `decodex.enforce_provider_attempt_binding()` | Require the exact initial route/plan/account/session/process/thread receipts and active revision-1 Turn for `initial_thread`; preserve other continuation branches. |
| `decodex.enforce_continuation_plan_completeness()` | Require the selected `L0` decision and complete first account/profile/session lineage for `initial_thread`; preserve same-thread and Context Pack branches. |

The latest schema does not roll an old body forward. It creates each final body once.
`decodex.enforce_routing_decision_completeness()` remains a separate unchanged semantic
boundary. No trigger is dropped, rebound, disabled, renamed, or used as a broad
starting-session bypass.

<a id="account-lifecycle-and-credential-authority"></a>

## Account lifecycle and observations

The account system has three owners:

1. Account Registry owns credential-negative product state.
2. HostCredentialStore owns versioned secret bundles.
3. Account Service coordinates enrollment, import, stable alias derivation, list,
   enable/disable, logout, refresh/rotation, app-server refresh callbacks, runner
   projection, account observations, recovery, and process pre-spawn checks.

The exact stable alias derivation and closed alias set remain owned by
[Account Lifecycle Authority](../specs/account-lifecycle-authority.md). Account Service
returns each alias. Clients present it and do not derive, rename, or own alias state.
Account UUID remains the row and routing identity.

One Codex process keeps one immutable Account UUID and provider binding for its lifetime.
Same-account token refresh may advance the credential version. It cannot switch accounts.
Credentials never enter PostgreSQL, public protocol data, process arguments, logs, or a
long-lived child environment.

The daemon account observer starts immediately, repeats on its bounded schedule, and
wakes after relevant account/effect changes. Different accounts progress concurrently.
Within one account, Reset Card work settles before profile observation so the profile uses
the exact successor credential. At most one observation owner is active per Account UUID;
additional wakes coalesce into one successor round. Slow work for one account does not
delay another.

Results publish progressively only against the current Account revision and cache
generation. A changed account invalidates its old Reset Card/profile state before a stale
in-flight result can publish.

`GetResetCards` and `GetAccountProfile` are isolated reads. They do not contact the
provider, start an app-server, join a refresh future, wait for an observation round, or
cause provider work. Profile reads use the latest persisted projection plus bounded
daemon refresh status. Reset Card reads use a revision-fenced daemon-lifetime public
cache. Restart discards that cache and reports typed retryable unavailability until the
new observation warms it.

The UI starts independent daemon value reads concurrently and keeps one bounded
`WaitForAccountObservation` query open instead of owning a second refresh clock. Each
daemon publication advances an opaque generation and wakes one coalesced cached-value
reload. Panel-open and manual triggers reload cached values only; they do not start
provider or app-server work.

The macOS UI uses its existing in-process Rust protocol client. `Refresh login` is its
only credential-replacement surface, and the native app ABI has no separate direct
credential-refresh command. Account Service and HostCredentialStore retain credential
verification, mutation, and refresh ownership. Swift does not stage or read credentials
and does not own provider observation. After successful login replacement, bounded
readback waits for the daemon's new revision-scoped observation instead of accepting an
old unauthorized value.

Candidate-5 changes must preserve this current-main observation/cache and UI/backend
behavior exactly.

## ProcessGeneration

ProcessSupervisor is the sole ProcessGeneration writer. A generation binds one account,
initial account revision, credential version/fingerprint, provider binding, exact-build
capability profile, execution epoch, attested launch manifest, boot identity, process
identity, state, revision, and timestamps. It stores no secret.

The durable states are `starting`, `ready`, `stopping`, `dead`, and `death_unknown`.
Only positive generation-bound evidence can establish `dead`. PID or process-group
absence, reuse, timeout, lease expiry, EOF, restart, identity mismatch, row absence, and
negative search do not prove death.

Startup projects every present nonterminal generation to `death_unknown`, performs one
bounded positive-only reconciliation pass, and continues background reconciliation.
Uncertainty blocks replacement only for the bound account. A restored process can be
observed but not adopted, proxied, reacquired, signaled, or terminated. Exact termination
requires the original unreaped child.

The execution-epoch authorization digest remains outside PostgreSQL. Database restore or
row replay cannot mint a fresh process fence. ProcessGeneration proves replacement
safety, not provider non-submission.

## ProviderAttempt

ProviderAttemptService is the sole external Codex turn-effect writer. One attempt binds
either a Conversation Turn or a ManagedRun execution to the accepted route decision,
continuation plan, RuntimeSession, selected account, ready ProcessGeneration, execution
epoch, request identity/digest, and provider correlation/idempotency keys.

The ordinary states are:

```text
prepared -> canceled | dispatch_authorized
dispatch_authorized -> succeeded | failed_definitive | not_submitted | unknown
unknown -> succeeded | failed_definitive | not_submitted
```

Terminal outcomes after authorization require positive evidence. Timeout, process death,
EOF, restart, lease expiry, absent rows, missing events, exhausted lists, and negative
search cannot establish `not_submitted`. A replacement can reconcile the original attempt
but cannot replay it. Late positive evidence remains attributable to the original attempt.

On restore, present `prepared` and `dispatch_authorized` attempts project to `unknown`.
Startup performs one bounded positive-only pass and background reconciliation continues
without starving later attempts.

## Stateless execution coordination

ExecutionCoordinator is a zero-sized crate-private sequencer. It retains no relation,
receipt, retry state, task, channel, lifecycle, account choice, RuntimeSession choice,
process state, or ProviderAttempt state. It consumes typed outputs from the sole owners
and returns an inert projection until a separately accepted product dispatch root exists.

Conversation remains the ordinary Turn owner. ManagedRun remains its own lifecycle and
acceptance owner. Routing Snapshot/Decision own eligibility and selection. Continuation
Plan owns first-session, same-thread, and Context Pack planning. ProcessSupervisor owns
process fences. ProviderAttemptService owns provider effects.

The protocol may expose read-only immutable execution-decision projection. It exposes no
route, session, process, attempt, wake, retry, receipt, or dispatch mutation.

## Managed repositories

PostgreSQL remains durable authority for each managed repository projection, monotonic
generation/tip, globally immutable operation assignment, append-only authority and
operation evidence, exact compare-and-swap, transaction completeness, and restart loads.
Git/filesystem execute admitted effects; GitHub supplies provider readback.

ManagedRepository assembly is optional and independent from `ProductStore` and Quick
Task. Absence is `Disabled`. A path, Git, executor, reconciliation, or isolated
configuration failure is typed `Unavailable` and disables repository operations only.
Neither state changes PostgreSQL readiness or prevents repository-free Quick Task work.

Each external operation has one complete canonical descriptor. Exact equality with an
existing assignment returns readback with no dispatch. Any difference is a permanent
operation-ID conflict. A fresh affine execution receipt exists only after successful
commit acknowledgement on the same live adapter path. Readback, restart, terminal state,
or an unknown commit outcome cannot reconstruct it.

Restart may perform only operation-specific read-only reconciliation. It never retries,
replays, adopts, repairs, or imports an external effect. These domain records remain
current product authority and are unrelated to schema upgrades.

## Paths, blobs, and protocol

`decodex-core` owns the typed `~/.decodex` root for configuration, logs, stable server
identity, SHA-256 blobs, and disposable cache. Repository paths and PostgreSQL socket
paths reject symbolic links and replacement according to their owner-specific
descriptor rules. Shared normal `~/.codex` remains Codex configuration, rollout, plugin,
and thread-visibility authority.

A requested Quick Task working directory is an input, not authority. Immediately before
spawn, the runtime host adapter opens and validates the selected path by no-follow
traversal, exact descriptor identity, directory type, ownership by the daemon effective
UID, and the applicable PostgreSQL or accepted host-only path policy. Ambient current
directory, repository discovery, and a caller-supplied path string cannot satisfy this
check. A failure rejects only that Quick Task command.

The exact-current protocol carries bounded command/result/event envelopes, snapshots,
history pages, account queries, Reset Card operations, and read-only diagnostics.
Unsupported protocol revisions receive typed refusal. Non-loopback binding remains
disabled until authentication, TLS, authorization, and redaction gates pass.

Blob-backed commands keep their receipt-first, exact-response, create-only publication
contract. PostgreSQL owns metadata and references; local CAS owns bytes. PostgreSQL alone
does not attest external bytes. Clients never receive local blob paths.

## Local development replacement

One reviewed operator-only action can stop the daemon and capture the complete
credential-negative reset tuple. For every retained account, the tuple contains Account
UUID, enabled state, account revision, provider binding, credential version/fingerprint,
and host-store binding. It also contains routing revision, mode, fixed target, and the
complete account order. The action can replace or directly transform the disposable
local database, run empty-target bootstrap when needed, restore or rebind that exact
tuple against unchanged host-vault credentials, prove exact readback, and restart the
daemon.

`fixed` mode requires one non-null target in the retained account set and order;
`balanced` mode requires a null target. The order is a complete duplicate-free
permutation of retained accounts, including disabled accounts, and readback proves each
account's exact enabled state together with the exact mode, target, and order.

For credential agreement, the action may invoke the existing `HostCredentialStore`
owner. The owner performs a confined in-process exact read, recomputes and compares the
credential fingerprint and binding, and returns only a typed credential-negative
agreement result. Neither the action nor its result may expose, serialize, copy, log,
persist, rotate, delete, or return token bytes. This is not a product or migration API,
generic attestation framework, metadata sidecar, generic importer, schema migrator,
backup/rollback path, receipt/finalizer, or fallback. Normal daemon startup never
performs it.

## Frozen provenance

`apps/decodex/`, Lane Authority v2, PR #1092, private-artifact documents, old numbered
schema evidence, old schema receipts, and storage-spike execution are historical only.
They may explain past behavior or a rejected design. They cannot define vNext runtime,
schema, command, test, or delivery authority.

When a historical invariant is still required, restate and prove it against the one
latest schema and current owner boundary. Do not revive its old version label, upgrade
path, compatibility mechanism, or executable schema owner.

## Change guidance

- Schema creation changes start in `crates/decodex-postgres/schema.sql` and the substantive
  `schema` module. Do not add another SQL owner.
- Runtime database changes must preserve zero-DDL startup and current-authority
  verification.
- Runtime composition changes must preserve independent ProductStore, Quick Task, and
  ManagedRepository projections. Do not add a mutable capability manager or silent
  optional startup path.
- Candidate-5 changes must preserve sole account selection, atomic first Turn/history
  admission, exact effect fencing, and current-main account cache-read isolation.
- ProcessGeneration changes must preserve positive-only death and account-local
  quarantine.
- ProviderAttempt changes must preserve positive-only outcome evidence and no replay.
- Account changes must preserve the Registry/store/service split and secret-negative
  database/protocol boundary.
- Configuration changes must keep transport/PostgreSQL parsing independent from optional
  repository configuration and must not restore duplicate repository-path authority.
- Historical migration evidence is provenance only. No acceptance claim may depend on a
  historical upgrade or ledger proof.

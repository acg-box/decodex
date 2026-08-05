# Decodex vNext Gate Manifest

Status: normative sequencing and acceptance boundary.

Owner: [vNext authority decision](../decisions/vnext-authority.md). Contract:
[vNext authority contract](vnext-authority.md).

## Changed decision

There are no external or deployed users. Local PostgreSQL state is disposable. Decodex
has one canonical unversioned latest schema and no supported schema migration or upgrade
path.

The old numbered SQL, Refinery runner, schema-history ledger, migration credential on
daemon startup, upgrade prefixes, migration receipts/finalizers/fallback, Phase A/B
schema receipts, and second executable schema owners are rejected. The old V14, V16,
V17, V33, and V34 labels do not name current owners. Their final accepted domain
semantics must be present directly in the latest schema.

Acceptance proves an empty-target bootstrap and exact current authority. It does not
prove a historical upgrade, populated migration, version prefix, schema-history checksum,
or source/restore migration receipt.

The third runtime-bootstrap candidate is donor source only. The next candidate must be
one integrated composition that keeps ProductStore, Quick Task, and ManagedRepository
readiness independent. It cannot be another runtime-lane-only patch.

## Delivery slices

| Slice | Usable result | Entry condition |
| --- | --- | --- |
| 1. Accounts and Quick Task | Mac account lifecycle subset, quota-aware initial fixed/balanced selection, explicit account order/manual recovery, Candidate-5 ordinary Quick Task, and minimal Accounts/Conversation/Health GPUI. | Latest-schema and runtime gates pass. The Slice-1 subset of MacDogfoodReady passes, including exact-build refresh callback, ProcessGeneration, ProviderAttempt, and Candidate-5 fences. |
| 2. Managed work | Project, Lead, global Advisor, bounded Context Revision, WorkItem, ManagedRun, repository saga, Task-Reviewer result, human acceptance, and Project/Work/Run GPUI. | Slice 1 and the managed-repository/ManagedRun owner gates pass. |
| 3. Self-hosting package | Representative two-account repository flow across restart boundaries and one Mac package. | Slice 2, package/restart/reconciliation, local database reset, and representative E2E evidence pass on one exact build. |

The dependency is `Slice 1 -> Slice 2 -> Slice 3`. An inert foundation cannot claim a
usable slice. Each acceptance records the exact source revision, evidence, contradictions,
and outcome.

Automatic cross-account same-thread fallback and all-depleted wake remain disabled under
XY-1304. They do not block Candidate-5 initial selection or the first Mac dogfood flow.

## Gate order

After one source candidate is frozen, run gates in this order:

1. integrated source-boundary and reverse scan for retired machinery, stale references,
   and duplicate owners;
2. fresh PostgreSQL 18 empty-target latest-schema bootstrap;
3. refusal of a second bootstrap against the same nonempty target;
4. runtime-only `decodexd` startup with zero DDL and no schema-owner credential;
5. independent ProductStore, Quick Task, and ManagedRepository startup projections;
6. exact current catalog/configured-authority verification and adversarial negative cases;
7. changed adapter SQL and domain behavior;
8. Candidate-5 behavioral boundaries, including current-main account observation/cache
   preservation;
9. direct local database replacement/rebind and exact credential-negative reset-tuple
   readback; and
10. the applicable Rust, transport, UI, packaging, and slice checks.

The exact hidden product commands are `decodexd bootstrap-latest-schema`,
`decodexd validate-current-authority`, and
`decodexd restore-local-account-authority`. The restore command has required `--root`
and `--schema-owner-user` options and the optional existing
`--schema-owner-credential-env-var` option. It reads its one transient document from
stdin.

## Integrated source boundary

Freeze Quick Task source before runtime integration. Then use one integration owner for
core configuration; runtime bootstrap, application, library, Quick Task, and
managed-repository modules; protocol doctor, Quick Task, wire, and library surfaces; and
the PostgreSQL latest-schema handoff.

The same owner removes shared acceptance drift from the root Cargo workspace,
`Cargo.lock`, task-runner definitions, deleted storage-spike references, and stale
migration/configuration fixtures. The frozen source candidate must load as one workspace
and must not require a deleted crate, removed command, rejected configuration field,
numbered schema, or migration-era fixture.

The third runtime-bootstrap candidate can donate reviewed source. It cannot supply
acceptance identity. Reject a fourth isolated runtime-bootstrap patch, a shared-file
handoff that is not integrated, or an exact-tree review that excludes root manifests,
task-runner files, lockfiles, and active fixtures.

## Latest-schema gate

### Canonical source

Acceptance requires exactly one executable schema source at
`crates/decodex-postgres/schema.sql`. It contains final enum, relation, constraint, index,
function, trigger, dependency, ownership, and ACL definitions directly.

Reject the candidate if it contains:

- a numbered SQL schema source or version allocation;
- Refinery or another migration runner;
- a schema-history relation, version constant, prefix verifier, or upgrade branch;
- DDL that exists only to alter, drain, backfill, convert, rename, or drop old Decodex
  state;
- compatibility reads/writes, rollback schema, migration receipt/finalizer, or fallback;
- a generated executable schema copy or parser-owned second schema;
- `SchemaManager`, registry, bootstrap facade, cutover coordinator, or generator pipeline;
  or
- executable schema ownership under `spikes/vnext-storage`.

The latest schema may include final required ownership/grant statements and current
domain integrity records. Exact-command receipts, account operations,
ProcessGeneration, ProviderAttempt, `schema_fingerprint`, runtime authority, activity,
outbox, and repository-effect evidence are valid only for their current domain contracts.
They must not be used as schema history.

### Module boundary

The `decodex-postgres` `schema` module passes only if it owns:

- the clean-target precondition;
- one transaction that executes the complete latest schema; and
- post-execution current catalog/configured-authority verification before commit.

A module that only exposes included SQL fails the gate. No caller may assemble partial
schema steps or invoke schema creation through normal store connection/startup.

### Empty-target bootstrap

Run against one fresh PostgreSQL 18 target with data checksums and the accepted verified
Unix-socket boundary. Prove:

- schema-owner credentials resolve only in the explicit operator invocation;
- endpoint directory, endpoint identity, expected server UID, and kernel peer UID are
  verified before authentication data is sent;
- the clean-target check requires the accepted fresh-database catalog baseline, apart
  from externally provisioned login roles, and rejects every user-created schema,
  relation, function, type, extension, or other product object;
- `pgcrypto` and every final Decodex object are created in one transaction;
- any SQL, catalog, authority, or verification failure rolls back the complete schema;
- exact post-execution verification passes before commit; and
- no source except `schema.sql` can create an accepted product catalog.

### Second-bootstrap refusal

Run the same bootstrap against the now initialized target. It must fail at the nonempty
precondition, execute no DDL, change no object or fingerprint, and return no success
receipt. There is no idempotent ensure/repair mode.

### Runtime-only startup

Start `decodexd` against the accepted target with only its runtime credential available.
Prove:

- no schema-owner credential is resolved, requested, inherited, or reachable;
- no DDL or extension/schema creation executes;
- no numbered SQL, migration source, schema-history relation, or bootstrap path is read;
- startup verifies the exact current catalog and configured authority;
- doctor/status performs the same bounded read-only revalidation;
- missing, extra, changed, unsafe, authentication-failed, or unreachable authority keeps
  product state typed unavailable; and
- endpoint replacement requires daemon restart and never causes repinning or repair.

### Runtime composition and readiness

Start one daemon and keep the one accepted endpoint. Exercise independent assembly
outcomes and prove:

- verified PostgreSQL produces `ProductStore::Available` even when Quick Task or
  ManagedRepository assembly fails;
- unavailable PostgreSQL produces `ProductStore::Unavailable` and persisted Quick Task
  reads return `ProductStateUnavailable`;
- Quick Task construction performs no I/O and cannot fail after validated ready
  dependencies are supplied;
- each missing or failed Quick Task dependency produces one immutable closed redacted
  `QuickTaskUnavailableReason`, and execute/start/resume return the typed unavailable
  result without hiding it through `.ok()`, an optional setter, or an omitted field;
- the initial user-supplied typed RoleProfile configuration bootstraps all four roles atomically,
  and a missing current `task` profile is a typed Quick Task initialization refusal;
- Quick Task unavailability does not remove diagnostics, account recovery, control-plane
  commands, or available PostgreSQL-backed reads;
- ManagedRepository absence is `Disabled`; repository-only configuration, path, Git,
  executor, or reconciliation failure is typed `Unavailable` and affects repository
  operations only;
- ProductStore, Quick Task, and ManagedRepository readiness are separate doctor/protocol
  fields and no result overwrites another;
- every Quick Task command repeats current owner fences, so startup readiness is not
  effect authority; and
- `AcceptanceUnknown` and recovery-required results are unchanged.

Configuration cases must prove that core transport/PostgreSQL parsing does not require a
static repository map. Missing or malformed isolated repository configuration cannot
block endpoint binding, PostgreSQL verification, or Quick Task. If a concrete host-only
repository policy remains, prove that it does not duplicate PostgreSQL identity,
admission, or persisted path policy.

For Quick Task spawn, prove exact no-follow working-directory traversal, descriptor
identity, directory type, ownership by the daemon effective UID, and accepted policy.
Reject ambient current directory, repository discovery, replacement, symlink, wrong
owner, wrong type, and unauthorized path. One unrelated broken repository must not
disable another Quick Task with a valid selected path.

## Current-authority gate

The accepted candidate must attest the exact current PostgreSQL 18 catalog. It covers:

- schemas, types/enums, relations, columns, defaults, constraints, indexes, sequences,
  functions, triggers, rules, policies, RLS state, and internal constraint triggers;
- all stable dependencies, including both sides of foreign keys and extension membership;
- exact owners, normalized ACLs/default ACLs, grantors, grant options, and role membership;
- function signatures, overloads, owner, language, security mode, volatility, parallel
  safety, settings, source bodies, dependencies, and execution grants;
- trigger names, bindings, timing/events, enabled state, constraint semantics, and bodies;
- the exact current `schema_fingerprint` and configured runtime authority; and
- semantic invariants for exact commands, history, blobs, accounts, routing,
  RuntimeSessions, ProcessGeneration, ProviderAttempt, managed repositories, and
  Candidate-5 first-session behavior.

Adversarial negatives must reject:

- PUBLIC authority or runtime relation/helper authority outside the closed contract;
- direct, inherited, NOINHERIT, membership-admin, or `SET ROLE` paths to ownership, DDL,
  `TRUNCATE`, trigger bypass, retention bypass, grant options, or role administration;
- unsafe default ACLs, overloads, function settings, hostile `search_path`, body changes,
  disabled/rebound/surplus triggers, rules, policies, or RLS drift;
- extension-member control, external cascades, indirect execution paths, sequence
  mutation, or extra grantees; and
- changed or forged `schema_fingerprint`/runtime-authority facts.

This gate derives the exact current inventory from the frozen latest candidate. It does
not reuse historical object counts or schema digests as authority.

## Adapter gate

Every adapter query and command must target the final latest-schema shape. Acceptance
covers:

- static reverse scan for old relation/function/enum names and old compatibility paths;
- prepared-statement parse and execution against the fresh latest-schema database;
- exact command request/response, idempotency, stable rejection, rollback, and
  concurrency behavior;
- history cursors, blob references, Context Packs, account profiles/quotas, routing,
  RuntimeSessions, ProcessGeneration, ProviderAttempt, managed repositories, and read-only
  diagnostics;
- startup/restart reconciliation that uses current domain records only; and
- no adapter access to schema bootstrap or schema-owner credentials.

Changed SQL is proved against the one fresh database and current catalog. There is no
old/new adapter compatibility matrix.

## Candidate-5 Quick Task gate

Candidate 5 remains target architecture. Candidate-4 tree
`f82b866e21f12742648023a2b468cc057afa52a1` is rejected provenance and cannot supply
implementation evidence.
This documentation amendment is not schema, code, migration, validation, or live-success evidence.

The integrated gate covers seven bounded behavior groups. They are test responsibilities,
not schema phases or separate authority frameworks.

1. **First and later Turn authority.** On a fresh latest-schema local database, establish six ready
   non-tombstoned accounts, canonical Account Registry routing, and the separately bootstrapped
   RoleProfiles. Prove the first RuntimeSession is selected and created with no Project, accepted
   Project policy, `routing_compatibility_evidence`, or `quota_windows` seed. On later Turns, prove
   `read_current_task_routing_authority_exact()` and `resolve_routing_snapshot_exact` are never
   called. The immutable
   non-selecting continuation decision must reference the current RuntimeSession, original initial
   decision, selected account, and copied Task RoleProfile. Same-thread and Context Pack retain
   those identities. Drift, exhaustion, or readiness failure returns typed manual recovery with no
   fallback, wake, or re-selection. ManagedRun still requires its complete Project-policy `L6`.
2. **Closed persistence and quota replay.** Prove `conversation_account_registry` initial `L0` and
   `managed_run_project_policy` `L6` are the only selecting snapshot shapes, and reverse constraints
   reject mixed consumers, fields, children, or evidence. For every Account Registry member, prove
   exactly one 300-minute and one 10080-minute slot and exact replay of: missing with all
   observation fields null; current with `used_percent`/`observed_at`/`resets_at` and no error; and
   observation error with typed `error_code`/`observed_at` and no usage/reset. Reject fabricated
   revision, remaining, confidence, provenance, or legacy quota values. Exercise fixed exact-target
   and balanced canonical-order selection with independent missing/current/error and 5-hour/7-day
   exhaustion. Replay only `selected`/`waiting`/`no_route` and exact exclusion reasons.
3. **Selection concurrency.** Prove one top-level `READ COMMITTED` exact command locks Conversation
   intent, routing control, canonical-UUID account rows, and current Task RoleProfile before it
   copies order and quota facts. Race it against enrollment, tombstone, order, fixed/mode, account
   observation, and quota writes. Prove routing writers take control before account rows, quota
   writers take account before quota and never control afterward, and an absent quota insertion
   waits on the account fence. Verify stale pre-commit facts roll back every effect. Same-key
   contenders replay one exact result; cross-key contenders produce one fresh selection and no
   duplicate snapshot/decision. For a later Turn, same-key contenders replay one continued binding
   and cross-key contenders create neither a second binding nor any selection work.
4. **Role, admission, and effect fences.** Prove typed configuration bootstraps all four
   RoleProfiles atomically, later updates remain in RoleProfile Authority, and missing current
   `task` returns `QuickTaskUnavailable` without synthesized defaults. Prove initial
   account/profile/session/plan and first Turn/Message atomicity. At every selected-account,
   ProcessGeneration, thread, and ProviderAttempt fence, prove drift or lost-result/restart state
   cannot re-select, spawn from non-fresh authority, duplicate an attempt, or fail the Turn. Only
   positive definite pre-effect refusal may create failed revision 2; explicit successor remains
   PostgreSQL-only non-dispatch evidence.
5. **Conversation and stream order.** Preserve result-before-stream order, one actor owner,
   bounded deferred publication, one command/registration slot, explicit publication
   source, non-reserving per-session acceptance, and no cross-session starvation.
6. **Transport completion.** Preserve duplicate/conflict command order, disconnect and
   reconnect, snapshot/cursor order, receiver-close outcomes, terminal result retention,
   one-shot peer-close finalization, task wake, and leak-free accounting. A local write or
   close does not prove peer receipt.
7. **Service shutdown.** Preserve `Accepting -> DrainingApplication -> DrainingEgress ->
   Closed`, one absolute deadline, admitted command completion, complete ordinal/transport
   reconciliation, exact session admission bounds including handshakes, zero surviving
   work, and cleanup only after closed accounting.

The latest schema must create the final accepted bodies and bindings for exactly these eight
affected trigger functions:

- `decodex.enforce_routing_completeness()`;
- `decodex.enforce_routing_decision_completeness()`;
- `decodex.enforce_runtime_session_state()`;
- `decodex.enforce_turn_state()`;
- `decodex.enforce_history_item_state()`;
- `decodex.enforce_provider_attempt_transition()`;
- `decodex.enforce_provider_attempt_binding()`; and
- `decodex.enforce_continuation_plan_completeness()`.

Prove the exact predicates in
[Quick Task thread establishment](vnext-authority.md#quick-task-thread-establishment).
No other trigger behavior, binding, or ACL may change. There is no broad starting-session
bypass.

Candidate-5 acceptance must preserve current-main account behavior: deterministic
one-word aliases, negative Reset Card counts as empty inventory, independent concurrent
per-account observations, Reset Card-before-profile ordering within an account,
coalesced successor rounds, revision-fenced publication, and query paths that read cache
or persisted projection without joining or starting provider refresh work.

## Domain-owner gates

ManagedRun may reach successful terminal completion only from explicit authoritative
WorkItem acceptance and validation. Objective achievement or evidence and any external
Codex Goal state cannot establish WorkItem acceptance or ManagedRun success.

### ProcessGeneration

Prove opaque launch identity, exact selected-account credential-negative binding, fresh
fence only, complete process identity, positive-only death evidence, account-local
quarantine, macOS exit-before-witness behavior, exact owned termination, restore epoch
safety, bounded reconciliation, and ProviderAttempt ambiguity handoff. Unsupported Linux
launch fails before profile-dependent preflights.

### ProviderAttempt

Prove both consumer shapes, atomic route/plan/session/process/Turn binding, every legal
state edge, positive-only terminal evidence, late-result attribution, restore projection,
replacement reconciliation without replay, duplicate-risk acknowledgement, bounded
background progress, and no second provider-effect ledger.

### Accounts

Prove Registry/HostCredentialStore/Account Service separation; versioned enable/mode/order
controls; exact credential CAS and reconciliation; exact-build refresh callback;
one-account-per-process binding; Reset Card fencing/replay; bounded profile/quota storage;
and current-main observation/cache-read isolation. No secret byte may enter PostgreSQL,
protocol data, logs, process arguments, or the local reset metadata.

### Managed repositories

Prove complete canonical operation descriptors, global operation-ID conflict, fresh
commit-acknowledged affine receipt, no receipt reconstruction, operation-specific read-only
restart reconciliation, exact Git/filesystem readback, and no schema/history role for
repository effect records.

Also prove independent service assembly: `Ready`, `Disabled`, and typed `Unavailable`
cannot alter ProductStore or Quick Task readiness. Repository configuration parsing is
isolated from core runtime configuration, and no static path map duplicates PostgreSQL
repository authority.

### Same-UID transport

Prove fixed owner-only endpoint publication, descriptor identity, peer UID checks, stale
recovery, replacement refusal, exact-current protocol, bounded session/command shutdown,
zero survivors before cleanup, and no TCP/loopback fallback.

## Local database reset gate

One reviewed operator-only action may:

1. stop the daemon and acquire the existing same-UID local transport namespace;
2. supply only credential-negative Account UUID, enabled state, account revision,
   provider binding, credential version/fingerprint, and host-store binding for every
   retained account, plus routing revision, mode, fixed target, and complete order;
3. run empty-target latest-schema bootstrap on the replacement database;
4. run `decodexd restore-local-account-authority --root ROOT
   --schema-owner-user USER [--schema-owner-credential-env-var ENV]` with the one strict
   `decodex/local-account-authority-restore/1` JSON document on stdin;
5. prove every exact host-vault binding before PostgreSQL mutation and again before
   commit;
6. prove every retained account, enabled value, revision, binding, and the exact routing
   revision/mode/fixed-target/order tuple; and
7. start the daemon after current-authority verification passes.

For `fixed` mode, the target is non-null and belongs to the retained account set and
complete order. For `balanced` mode, it is null. The order is a duplicate-free
permutation of all retained accounts, including disabled accounts. Readback must reject
any changed enabled value, mode, target, order, revision, or membership.

For credential agreement only, the existing `HostCredentialStore` owner may perform a
confined in-process exact read, recompute and compare the credential fingerprint and
binding, and return a typed credential-negative agreement result. Acceptance proves that
the operator action and result do not expose, serialize, copy, log, persist, rotate,
delete, or return token bytes. The action creates no public product or migration API,
generic attestation framework, metadata sidecar, generic importer/migrator, backup,
rollback, receipt/finalizer, compatibility path, or fallback. Failure before exact
readback leaves the daemon stopped.

The command refuses a non-fresh target. Apart from the initial routing singleton and the
one active bootstrap execution epoch, all ordinary tables are empty and all identity
sequences are untouched. The command writes only current account rows, account order,
and routing control. The stdin document is bounded to 512 KiB and 512 accounts, rejects
unknown or duplicate fields, has no display labels, and is never persisted. Output has
only a closed `classification` and `account_count`.

## Mac dogfood gate

MacDogfoodReady requires:

- a fresh latest-schema PostgreSQL 18 database or the accepted local reset result;
- the signed daemon wrapper, exact Keychain identity/access group, and same-UID transport;
- Account Registry/HostCredentialStore exact bindings and routing controls;
- all four global RoleProfiles from the atomic typed configuration bootstrap, including a current
  `task` profile;
- exact-build account login/refresh callback proof;
- independent 300-minute and 10,080-minute quota facts;
- current-main account observations, Reset Card/profile readback, and terminal Reset Card
  replay;
- Candidate-5 Account Registry initial routing without a Project/policy/evidence seed, plus
  process/thread/attempt fences;
- minimal Accounts/Conversation/Health GPUI; and
- packaged startup with zero DDL, no schema-owner credential, no old watcher,
  environment credential projection, helper, `:8192`, or old database input.

Linux credential storage, broad profile/history presentation, automatic fallback/wake,
retained-title Desktop discovery, remote access, graph/automation, and polish remain
later obligations unless a slice explicitly names them.

<a id="xy-1372-private-artifact-capability-and-consumption-gate"></a>

## Historical evidence

Old migration-based PostgreSQL evidence, frozen schema manifests, S0/R1/R2 captures,
Phase A/B receipts, numbered-ledger checks, version-specific matrices, private-artifact
phases, Lane Authority v2, PR #1092, and v0.2 state are superseded provenance. They can
suggest invariants and hostile cases only.

No historical file can authorize a current command, schema source, upgrade proof, second
owner, or acceptance claim. A retained invariant must be restated and pass against the
one latest schema and current authority.

## Stop conditions

Stop the owning gate on:

- any second executable schema source or schema-creation path;
- any daemon startup DDL or schema-owner credential resolution;
- any accepted bootstrap on a nonempty target;
- any current-authority check that depends on schema history or an upgrade prefix;
- any secret byte in database/reset metadata, protocol data, logs, or process arguments;
- daemon-fatal Quick Task or ManagedRepository startup when the control plane can start;
- a mutable capability manager, silent optional Quick Task disappearance, or readiness
  state that substitutes for current owner fences;
- repository configuration that blocks core parsing or duplicates PostgreSQL path
  authority;
- a Quick Task dependency on Project policy, `routing_compatibility_evidence`, or `quota_windows`;
  a later-Turn snapshot/selection call; or a compatibility bridge that keeps duplicate routing
  paths;
- a second account selector, provider-effect ledger, process owner, or coordinator state;
- possible external-effect replay without positive reconciliation;
- Candidate-5 partial lineage, partial admission, ambiguous terminalization, or account
  observation/cache regression;
- a second mutation path around `decodexd`;
- a source candidate that retains deleted workspace/task-runner references or active
  migration/configuration fixtures;
- unbounded UI history loading; or
- remote binding before security acceptance.

A decision-level contradiction requires an explicit architecture revision. It never
authorizes a compatibility facade, silent fallback, generic migrator, or extra phase.

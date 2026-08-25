---
type: "Historical Architecture Decision"
title: "Decodex vNext Authority Decision"
description: "Historical former server-store authority decision retained for rationale and domain invariants; storage, migration, and delivery authority is superseded by the SQLite local-product decision."
tags: [history, architecture, authority, superseded]
---

# Decodex vNext Authority Decision

Status: historical former server store design record. Its storage, migration, and delivery
authority is superseded by the
[SQLite local-product decision](sqlite-local-product.md). Retain this page only to
explain earlier choices and reusable domain invariants.

Owner issue: [XY-1260](https://linear.app/hack-ink/issue/XY-1260/promote-the-vnext-authority-contract-and-supersede-lane-authority-v2)

## No-migration reset

There are no external or deployed Decodex users. Current local former server store data is
disposable development state. Decodex therefore has no supported database-upgrade
contract and no compatibility obligation to an older Decodex catalog.

This instruction changes the context of the accepted vNext product decision. The
continuity relation is `same_decision_changed_context`: the product and Candidate-5
Quick Task remain the target, but the old versioned schema plan is superseded.

Decodex vNext has exactly one canonical, unversioned latest former server store schema source:
`crates/decodex-server-store/schema.sql`. That file contains the final definitions for all
accepted enums, relations, constraints, indexes, functions, triggers, dependencies,
owners, and grants. It contains no old-state branch, drain, backfill, compatibility
definition, or reverse operation. A definition that exists only to transform an older
Decodex catalog has no place in the latest schema.

The following mechanisms are rejected product and repository architecture:

- numbered schema SQL files and version constants;
- Refinery or another schema migration runner;
- a schema-history relation, ordered schema ledger, or upgrade-prefix check;
- upgrade branches, compatibility DDL, migration receipts, finalizers, or fallback;
- Phase A or Phase B schema-migration receipts and source/restore digest transitions;
- a generated second executable schema owner;
- a `SchemaManager`, schema registry, generator pipeline, bootstrap facade, or cutover
  coordinator; and
- the executable schema owner in `spikes/vnext-storage`.

Delete the old machinery directly. Do not add a migration that removes migrations.
Historical evidence that refers to migrations remains provenance only. In particular,
the old V14, V16, V17, V33, and V34 labels and their allocation are rejected. Their
accepted final domain semantics must be expressed in the latest schema and current
owner names.

`decodex-server-store` owns the one schema source and exact current-authority verification.
A Rust `schema` module is allowed only when it owns all three substantive bootstrap
responsibilities: the clean-target precondition, one transactional schema execution,
and post-execution verification. A module that only wraps `include_str!` is not an
owner and is prohibited.

Normal `decodexd` startup is runtime-only. It resolves no schema-owner credential,
executes zero DDL, and does not create, alter, repair, or upgrade a catalog. It connects
with the configured runtime identity and verifies the exact current catalog and
configured authority. Any missing, extra, changed, unsafe, or unreachable authority
keeps product state unavailable.

Schema creation is one explicit operator action against an empty former server store 18 target.
That action resolves the schema-owner credential, proves the clean-target precondition,
executes the complete latest schema in one transaction, verifies the resulting catalog
and configured authority, and commits only when every check passes. A second bootstrap
against that now nonempty target fails closed. Bootstrap is not daemon startup.

## Preserved former server store boundary

The reset keeps former server store 18, data checksums, `pgcrypto`, and the verified Unix-socket
boundary. It also keeps exact attestations for:

- schemas, relations, columns, types, functions, triggers, constraints, indexes, and
  stable dependencies;
- object ownership, role reachability, function settings and bodies, ACLs, and runtime
  callable surfaces;
- configured runtime authority and the negative PUBLIC/runtime contract;
- hostile `search_path`, overload, default-ACL, extension-membership, external-cascade,
  trigger, rule, policy, RLS, grant-option, DDL, `TRUNCATE`, sequence, and retention
  bypass cases; and
- the pinned socket directory, socket identity, peer UID, former server store major, checksums,
  and required extension.

The reset removes only history, upgrade, prefix, and migration-source predicates. The
current verifier compares the live catalog with the one latest schema contract. It does
not compare a history table or parse a sequence of old SQL files.

Domain integrity records remain. Exact-command receipts, account-operation records,
ProcessGeneration, ProviderAttempt, `schema_fingerprint`, runtime authority, activity,
outbox, and repository-effect evidence protect current product behavior. None is a
schema migration mechanism, schema-history substitute, or bootstrap permission.

## Local development reset

One reviewed operator-only local action may preserve credential-negative account,
routing, and binding metadata while it replaces the disposable database. The action
may:

1. stop `decodexd`;
2. capture only the reviewed credential-negative Account UUID, enabled state, account
   revision, provider binding, credential version/fingerprint, and host-store binding for
   every retained account, plus routing revision, mode, fixed target, and complete order;
3. drop and recreate the local database, or directly transform that one local database;
4. run the one empty-target latest-schema bootstrap when the database was recreated;
5. restore or rebind the exact captured tuple against the unchanged host-vault records;
6. prove every Account UUID, enabled state, account revision, provider binding,
   credential version/fingerprint, host-store binding, routing revision, mode, fixed
   target, and order exactly; and
7. start `decodexd` only after current-authority and binding readback pass.

For `fixed` mode, the fixed target is non-null and belongs to the retained account set
and complete order. For `balanced` mode, it is null. The order is one duplicate-free
permutation of all retained accounts, including disabled accounts. Restoration and
readback prove the per-account enabled values and the routing mode, target, and order as
one coherent tuple.

Credential agreement may invoke the existing `HostCredentialStore` owner to perform a
confined in-process exact read and recompute and compare the credential fingerprint and
binding. It returns only a typed credential-negative agreement result. The operator
action and result must never expose, serialize, copy, log, persist, rotate, delete, or
return token bytes. Host-vault credentials remain in place and unchanged.

This is a finite local development action. It is not product account migration, a
generic migrator, an import API, a generic attestation framework, a metadata sidecar, a
backup or rollback mechanism, a receipt/finalizer, or a fallback. Normal installation
and startup do not read an old database or an old account source.

## Runtime composition reset

Three runtime-bootstrap candidates failed source review because they coupled unrelated
owners, hid Quick Task startup failure, or left shared integration drift. The third
candidate is a source donor only. It is not an accepted runtime candidate and it must not
receive another runtime-lane-only patch.

This reset keeps one `decodexd`, one owner-only same-UID endpoint, and one application
surface. The daemon can start when Quick Task execution or ManagedRepository is not
available. former server store-backed reads, diagnostics, account recovery, and the control plane
remain available when their own owners are ready.

Runtime composition has three independent startup results:

1. `ProductStore` means verified former server store only. Its result is
   `Available(PostgresStore)` or `Unavailable(ProductStateReason)`. Quick Task,
   repository, Git, path, or reconciliation failure cannot replace or erase this result.
2. `QuickTaskRuntime` construction is infallible and performs no I/O after all fallible
   dependency owners return validated ready dependencies. Composition records one
   immutable startup projection: `Ready(QuickTaskRuntime)` or
   `Unavailable(QuickTaskUnavailableReason)`. The unavailable reason is closed and
   redacted.
3. ManagedRepository is an independent optional capability. Its startup projection is
   `Ready`, `Disabled`, or `Unavailable` with a closed redacted reason. Repository path,
   Git, reconciliation, or isolated configuration failure affects repository operations
   only.

These results are startup projections, not mutable authority. They create no capability
manager, lifecycle, receipt, cached substitute, or recovery framework. Every command
repeats the accepted checks of the former server store, Account Service, ProcessGeneration,
ProviderAttempt, app-server, path, and repository owners that apply to that command.

Core runtime, transport, and former server store configuration are independent from optional
repository configuration. Remove the required static `server_host.repositories` path map
unless a concrete accepted host-only policy consumes it. former server store remains authority for
repository identity, admission, and persisted path policy. If a host-only repository
policy remains, it has a separate parser and validator. Its absence or invalid content
cannot block former server store verification or Quick Task assembly.

Immediately before spawn, the runtime validates the selected Quick Task working directory
by exact descriptor identity, directory type, ownership by the daemon effective UID,
no-follow traversal, and the applicable accepted path policy. Ambient current directory
and repository discovery grant no authority. One unrelated broken repository cannot
disable all Quick Tasks.

Protocol and doctor project `ProductStore`, Quick Task, and ManagedRepository readiness
separately. Quick Task execute, start, and resume return typed
`QuickTaskUnavailable(reason)` when the immutable startup projection is unavailable.
Persisted Quick Task reads keep `ProductStateUnavailable` when former server store is unavailable.
No `.ok()` conversion, optional setter, or absent field may hide a startup failure.
`AcceptanceUnknown` and recovery-required results keep their existing meanings.

The next source candidate has one integration owner after the Quick Task source freezes.
That owner reconciles core configuration; runtime bootstrap, application, library,
Quick Task, and managed-repository owners; protocol doctor, Quick Task, wire, and library
surfaces; root Cargo/task-runner/lock files; deleted storage-spike references; and stale
migration/configuration fixtures. This is integration acceptance-source work. It is not a
fourth runtime-bootstrap patch.

Daemon-fatal Quick Task startup, separate serve profiles, a mutable capability manager,
duplicate repository-path authority, and another isolated runtime-lane candidate are
rejected. This reset does not change the one-latest-schema or no-migration authority.

## Candidate-5 Quick Task

Candidate 5 remains the accepted architecture for one ordinary Quick Task. It is a
multi-turn Conversation with no WorkItem, ManagedRun, reviewer, PR, harness, or Goal.
Candidate-4 tree `f82b866e21f12742648023a2b468cc057afa52a1` remains materially
rejected provenance.

Scoped supersession: the earlier Candidate-5 requirement for Project policy order and
capability facts does not apply to ordinary Quick Task. Account Registry authority selects once
when the first RuntimeSession is established. ManagedRun Project-policy authority is unchanged.

Candidate 4 failed because it did not close five authority boundaries:

1. effect and successor fences did not lock the exact selected Turn and revision;
2. the first Turn and first history item were not one atomic, one-winner Conversation
   transaction;
3. ambiguous ProcessGeneration replay could terminalize a Turn;
4. the required narrow trigger behavior contradicted an active-only trigger rule; and
5. Account Service and routing could select different accounts.

Candidate 5 uses existing owners in this exact order:

1. Conversation authority creates the Conversation.
2. Routing receives one prospective Turn UUID as intent. No Turn row or Turn foreign
   key exists at this point.
3. The Quick Task routing adapter locks complete Account Registry membership, canonical routing
   control, exact account facts and blockers, the current Task RoleProfile revision, and two exact
   Account Registry quota slots per member.
4. Routing Decision authority runs once and is the sole Quick Task account selector.
5. Continuation Plan authority consumes that selected decision and atomically creates
   the selected account snapshot, copied RoleProfile snapshot, first revision-1
   unfenced `starting` RuntimeSession, inert `initial_thread` plan, exact receipt,
   activity, and outbox.
6. Conversation authority atomically admits the exact prospective UUID as the active
   revision-1 sequence-1 user Turn and creates exactly one ordinal-0 completed Message.
7. Account Service rechecks only the selected account's readiness, credential version
   and fingerprint, provider binding, and HostCredentialStore binding immediately
   before spawn. It cannot select or substitute an account.
8. ProcessSupervisor may spawn only from a fresh ProcessGeneration fence.
9. RuntimeSession Thread Establishment owns the exact thread-start fence, start binding,
   activation, and acknowledgement.
10. ProviderAttemptService owns attempt preparation, dispatch authorization, ambiguity,
    positive evidence, and reconciliation.

Runtime is a stateless sequencer across these owners. It does not become an account,
route, session, Turn, process, thread, or provider-effect authority.

Selecting snapshots have two closed authority shapes. `conversation_account_registry` is the
initial Quick Task `L0` shape and has null Project policy/evidence/build fields.
`managed_run_project_policy` is the existing ManagedRun `L6` shape and keeps its complete Project
policy/evidence/quota contract. Reverse-shape constraints reject mixed fields.

The Account Registry shape binds exact routing revision/mode/fixed target/order, current Task
RoleProfile revision, every non-tombstoned member and blocker, and duration-keyed 300-minute and
10080-minute quota slots. Each slot exactly preserves `account_quota_facts` as missing, current
(`used_percent`, `observed_at`, `resets_at`), or observation error (`error_code`, `observed_at`).
It fabricates no revision, remaining value, confidence, or provenance.

There is exactly one selecting route decision for an ordinary Quick Task. Each later Turn creates
an immutable non-selecting `conversation_continuation` decision binding to the current
RuntimeSession, its original
initial decision, selected account snapshot, and copied Task RoleProfile snapshot. It does not
call `read_current_task_routing_authority_exact()`, resolve another Account Registry snapshot, or
run selection.
Same-thread continuation and Context Pack fallback retain that exact account and profile.
Selected-account drift, exhaustion, or readiness failure returns typed manual recovery without
fallback, wake, alternate account, or re-selection. Automatic cross-account fallback and
all-depleted wake remain disabled under XY-1304.

Every process, thread, and provider-effect fence locks and rechecks the exact selected
Turn as active revision 1 under the same Conversation and first RuntimeSession.
ProcessGeneration and thread establishment through bind require the applicable
`starting` RuntimeSession revision. ProviderAttempt preparation and authorization
require the exact post-bind `active` revision and exact thread fence/bind receipts.

Only a fresh ProcessGeneration result can spawn. Replayed, rejected, or uncertain state
returns durable readback or `Unknown`; it cannot spawn, replace, adopt, create a
successor, prepare a duplicate attempt, or terminalize the Turn. Conversation authority
may move the exact Turn to `failed` revision 2 under a starting session only after
positive proof of a definite pre-effect refusal. Any ambiguous or started effect keeps
the Turn active for manual recovery.

Explicit successor remains former server store-only, non-dispatch evidence. Before any write, it
locks the Turn named by the selected decision and requires that Turn to be in the same
Conversation and source RuntimeSession, `failed`, and revision 2. It has no protocol
field, product command, runtime grant, facade, fallback, or wake path.

The latest schema defines the final forms of the eight affected trigger functions
directly:

- `decodex.enforce_routing_completeness()`;
- `decodex.enforce_routing_decision_completeness()`;
- `decodex.enforce_runtime_session_state()`;
- `decodex.enforce_turn_state()`;
- `decodex.enforce_history_item_state()`;
- `decodex.enforce_provider_attempt_transition()`;
- `decodex.enforce_provider_attempt_binding()`; and
- `decodex.enforce_continuation_plan_completeness()`.

Their accepted narrow predicates are normative in the
[vNext authority contract](../specs/vnext-authority.md#quick-task-thread-establishment).
The latest schema does not alter old trigger bodies. It creates these final bodies and
bindings. Every unrelated write keeps the existing active-only behavior. No broad
starting-session bypass is allowed.

Candidate 5 must preserve current-main account observation behavior: independent
per-account provider refresh, Reset Card before profile within one account, concurrent
progress across accounts, coalesced successor rounds, revision-fenced publication, and
query paths that read daemon-owned cache/projection state without joining or starting
refresh work. It also preserves deterministic one-word account aliases, current account
and UI behavior, negative Reset Card counts as an empty inventory, automation behavior,
same-thread and Context Pack authority, and XY-1304 containment.

## Product decision

Decodex vNext is a rebuild of the agent workspace, not an incremental extension of the
frozen Linear-lane and SQLite runtime. The normative contract is
[vNext Authority](../specs/vnext-authority.md). Ordered acceptance is in
[vNext Gates](../specs/vnext-gates.md).

Within vNext work, authority descends in this order:

1. explicit user direction and checked-in project policy;
2. this decision, the vNext authority contract, and the vNext gate manifest;
3. accepted domain and protocol contracts created under those documents;
4. source, tests, the one latest schema, and operational runbooks that implement an
   accepted gate;
5. OpenWiki navigation and current-source descriptions; and
6. Linear plans, historical evidence, and research as provenance.

Target documents do not claim that target behavior is already implemented. Current
source does not override a later accepted target. A contradiction stops the affected
work for an explicit decision.

## Accepted shape

- former server store owns Decodex product state. It uses transactions, exact commands, leases,
  append-only activity, and a transactional outbox. It is not event sourced.
- A shared normal `~/.codex` owns persistent Codex rollout and thread visibility.
  Decodex maps only threads that it created.
- `decodexd` alone owns scheduling, app-server children, product mutations, repository
  side effects, and adapters. GPUI, SwiftUI, CLI, and MCP are clients.
- `ProductStore` represents verified former server store only. Quick Task and ManagedRepository
  startup projections are independent and cannot overwrite product-store readiness.
- former server store Account Registry owns credential-negative account state. One
  HostCredentialStore owns secret bundles. On macOS, its only normal adapter is the
  daemon-owned redb file at `~/.decodex/server/credentials.redb`. Account Service
  coordinates account operations.
- One app-server process remains bound to one Account UUID and provider identity for its
  lifetime. Credentials do not switch accounts in a live process.
- former server store remains the complete routing-fact and decision authority. Runtime and clients cannot
  supply the account universe, eligibility, Account Registry order, Project-policy order,
  selection, continuation binding, or exclusions.
- ProcessSupervisor and ProviderAttemptService retain separate replacement-safety and
  external-effect-safety authority.
- ExecutionCoordinator remains stateless and cannot authorize dispatch by itself.
- Git/filesystem own repository bytes and worktrees. GitHub owns PR/check/merge
  readback. former server store owns the admitted repository-effect state and evidence.
- Quick Task construction is I/O-free and infallible after validated dependencies exist.
  Its startup projection is immutable, typed, and visible through protocol diagnostics.
- Local content-addressed storage owns large bytes; former server store owns their metadata and
  references.

Delivery uses three vertical slices:

1. Accounts, Candidate-5 Quick Task, and minimal Accounts/Conversation/Health GPUI.
2. Project, Lead, global Advisor, Context Revision, WorkItem, ManagedRun, repository
   saga, Task-Reviewer result, human acceptance, and Project/Work/Run GPUI.
3. One representative two-account self-hosting flow across restarts and one Mac package.

Automatic cross-account fallback and all-depleted wake are later work. They do not block
Candidate-5 initial selection or the first Mac dogfood flow.

## Supersession

Lane Authority v2, the frozen v0.2 runtime, PR #1092, and the private-artifact program are
historical provenance. They are not vNext implementation or runtime inputs. The
[private-artifact archive](../specs/private-artifact/README.md) remains evidence only.

Old migration-based former server store evidence may still explain a domain invariant or a past
failure. It cannot authorize numbered SQL, a schema ledger, an upgrade proof, or a
second schema owner. Any useful invariant must be restated under the latest-schema and
current-authority gates before implementation relies on it.

## Decision falsifiers

Stop and return to the authority owner if evidence shows that:

- one latest empty-target schema cannot express the accepted product safely;
- current catalog and configured authority cannot be verified without a history ledger;
- normal daemon startup requires DDL or a schema-owner credential;
- the local credential-negative rebind cannot preserve exact host-vault bindings without
  exposing token bytes outside the `HostCredentialStore` owner or changing them;
- Candidate-5 owners cannot compose without a second account selector, coordinator
  state, pre-admission Turn row, or ambiguous-effect replay;
- ordinary Conversation execution requires a ManagedRun;
- ProviderAttempt must create or rewrite RuntimeSession state;
- ProcessGeneration uncertainty must be treated as provider non-submission; or
- a hostile same-UID or multi-tenant requirement makes the accepted single-host trust
  boundary invalid.

A falsifier blocks the affected gate. It does not authorize a compatibility facade,
silent fallback, generic migrator, or second authority path.

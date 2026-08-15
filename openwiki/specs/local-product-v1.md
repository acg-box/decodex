---
type: "Specification"
title: "Local Product V1 Contract"
description: "Normative SQLite milestone contract for app-server freshness, Quick Task execution, lifecycle safety, and deferred product capabilities."
tags: [local-product, sqlite, quick-task, app-server]
openwiki:
  roles: [architecture, domain, workflow]
  change_kinds: [lifecycle, public-api, runtime]
  source_paths: [crates/decodex-runtime/src/quick_task.rs, crates/decodex-runtime/src/account_launch/process.rs, crates/decodex-protocol/src/quick_task.rs]
  symbols: [QuickTaskExecutionSettings, QuickTaskRecoveryAction, control_thread]
  test_paths: [database/tests/quick_task_restart.rs]
  invariants: [Exact thread lifecycle readback precedes local archive commit.; Fast is request-scoped and never mutates global Codex configuration.; A live account process generation rejects a second request before provider effect.]
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
Opening a Conversation refreshes that selected thread. The explicit sidebar sync builds
a bounded client-side batch and applies the same command sequentially to every local
provider-backed Conversation, then reloads the SQLite list. It does not add a bulk
provider API or a second state authority. The current V1 refresh contract covers thread
lifecycle. If exact app-server readback reports that the thread is archived, one SQLite
transaction archives the Conversation and ends its active RuntimeSession. Decodex
removes that Conversation from the active task list. A Decodex archive command uses
exact pre-read, archive, and post-read in one account-bound app-server process before it
commits the same local transition. The runtime refuses refresh/archive while a turn,
establishment, or unresolved provider attempt is active, and commits the local archive
only after the exact provider result is positive and the expected Conversation and
RuntimeSession revisions still match. The sidebar reports these definite refusals as
skipped instead of fabricating a provider outcome.

The local Conversation list is a SQLite read and does not assert current provider
freshness. Opening a task or explicitly syncing the sidebar requests provider readback.
V1 does not continuously poll every account and thread. App-server
`thread/read(includeTurns=true)` exposes visible but lossy history; it is not complete
history or tool-effect authority. V1 therefore does not import arbitrary external
turns during lifecycle refresh. A later history-merge contract must define identity,
completeness, provenance, and effect safety before it can update normalized history.

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
- routing decisions and inert continuation plans;
- RuntimeSession snapshots and Codex-thread establishment evidence;
- ProcessGeneration intent, exact identity, state, and positive death evidence; and
- ProviderAttempt preparation, dispatch authorization, unknown projection, and positive
  terminal evidence.

Large history content continues to use the content-addressed blob owner. SQLite stores a
bounded inline value or a digest and length.

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
15. Provider archive readback can close a local Conversation only when no Turn or
    ProviderAttempt is unresolved and exact Conversation and RuntimeSession revisions
    still match.

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
- ontology and graph projections;
- remote workers and multi-machine coordination; and
- cross-conversation app-server process multiplexing; and
- Context Pack fallback when exact same-thread proof is absent.

## Acceptance

Acceptance requires:

- a fresh installation that needs no separate database server, redb, or Keychain runtime access;
- exact database initialization, reopen, migration, inventory, and integrity checks;
- bounded idempotent transfer of the current account pool with source retention;
- one real Codex app-server response;
- a daemon restart followed by a later response on the same Conversation and Codex
  thread, with no duplicate ProviderAttempt dispatch;
- protocol-only GPUI and CLI operation;
- exact selected-thread refresh, verified archive, and request-scoped execution controls;
- focused and workspace-wide tests; and
- current OpenWiki and local database gates.

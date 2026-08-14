# Local Product V1 Contract

Status: normative contract for the current SQLite milestone.

Owner: [SQLite local-product decision](../decisions/sqlite-local-product.md).

## Product boundary

`decodexd` is the sole owner of product state and external effects. Clients use the
same-UID Unix WebSocket protocol. They do not open product storage or credential files.
Codex app-server remains the provider execution kernel, with one Codex thread bound to
one Decodex RuntimeSession.

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
12. V1 permits one non-dead ProcessGeneration per account. A second initial request for
    that account fails before provider effect with `RestoreProcessReadiness`; it must not
    become acceptance ambiguity.
13. Account affinity is scoped to a Conversation. Its initial Routing Decision binds the
    RuntimeSession and Codex thread to one account. Later Turns do not re-evaluate global
    routing. Independent Conversations can bind to different accounts.

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
after the daemon restarts. If the bound account becomes depleted, V1 fails closed instead
of switching the Conversation in place and losing provider cache affinity.

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
- focused and workspace-wide tests; and
- current OpenWiki and local database gates.

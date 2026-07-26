# Account Lifecycle Authority

Status: normative vNext account authority and clean-cutover contract for XY-1423.
XY-1422 owns implementation. Production account routing, account UI acceptance,
whole-product acceptance, and legacy removal remain blocked until XY-1422 passes.

This contract supersedes any text that treats the environment-backed credential
projection or the continuously watched legacy `accounts.jsonl` bridge as a complete
vNext account pool. Those paths are pre-cutover scaffolding only.

## Scope and invariants

The final account system has exactly three owners:

1. The PostgreSQL Account Registry owns credential-negative product state.
2. A narrow HostCredentialStore owns versioned secret bundles.
3. The `decodexd` Account Service coordinates every account operation.

GPUI, SwiftUI, CLI, MCP, PostgreSQL, and the long-lived daemon environment never own
credential bytes. They receive only redacted account projections, operation states,
and typed results. No client protocol payload carries an access token, refresh token,
identity token, API key, or import file content.

The normal shared `~/.codex` remains Codex authority for configuration, plugins, and
rollout files. It is also the storage that makes Decodex-created Codex threads visible
to standalone Codex. Decodex does not scan or import Codex thread history.

One Codex process has one immutable Account UUID and provider identity for its complete
lifetime. The Account Service can return a newer access token for that same account in
response to an app-server refresh callback. It can never bind the process to another
account.

## Ownership model

### PostgreSQL Account Registry

The Account Registry owns:

- the stable Account UUID;
- label, enabled state, routing state, revision, and user-owned routing order;
- provider kind and credential-negative provider identity;
- separate 300-minute and 10080-minute quota observations and exclusions;
- capability and account-health projections;
- usage, profile, and bounded history projections;
- the current non-secret credential version and fingerprint;
- account-operation intents, states, idempotency receipts, and reconciliation results;
- active ProcessGeneration and ProviderAttempt bindings through their existing owners;
- one-shot migration identity and item results.

PostgreSQL does not store a credential, an encrypted credential blob, a secret-store
locator that grants retrieval, or an ambient Codex auth export. A credential fingerprint
is equality and reconciliation evidence only. It must not contain enough information to
retrieve a secret.

The routing account state remains separate from credential state. An account with an
unsettled credential operation, unavailable secret backend, missing exact credential
version, or failed provider binding is not eligible even if its last routing state was
`available`.

### HostCredentialStore

The HostCredentialStore owns one record per Account UUID. Its secret bundle contains:

| Field | Contract |
| --- | --- |
| `schema_version` | Closed bundle-format version. |
| `account_id` | Exact vNext Account UUID. |
| `provider_kind` | Closed provider kind, initially ChatGPT. |
| `provider_account_id` | Exact provider identity bound to the tokens. |
| `credential_version` | Positive monotonic version for compare-and-swap. |
| `writer_operation_id` | Account-operation UUID that wrote this version. |
| `access_token` | Current access token. |
| `refresh_token` | Current refresh token. |
| `id_token` | Current identity token when the provider supplies or requires it. |
| `token_metadata` | Only provider-required token type and expiry data. |

The store returns only non-secret metadata to reconciliation callers: Account UUID,
provider binding, credential version, writer operation ID, and a domain-separated
fingerprint of the complete canonical bundle. Debug output and errors redact all secret
fields.

The port has only these mutation primitives:

- create when the Account UUID is absent;
- read exact metadata or read the bundle for one daemon-owned operation;
- compare-and-swap from one exact credential version to the next;
- delete one exact credential version;
- report a typed backend capability state.

Create, compare-and-swap, and delete are atomic for one Account UUID. A stale version,
wrong provider binding, duplicate provider identity, missing item, or backend failure is
a typed result. There is no list-all-secrets operation in a client surface and no
fallback to environment variables or a legacy file.

### macOS adapter

The macOS adapter uses Keychain generic-password items under the Decodex product
identity:

- application identity: `box.acg.decodex`;
- service namespace: `box.acg.decodex.credentials.v1`;
- Keychain account name: canonical vNext Account UUID;
- synchronizable: false;
- accessibility: after first unlock, this device only.

Only the signed Decodex daemon identity can read or mutate these items. Installer,
SwiftUI, GPUI, and CLI processes call the Account Service and do not receive direct
Keychain access. The adapter must support unattended refresh after the first device
unlock, so it must not require user presence for each token rotation. A locked Keychain,
invalid signing requirement, denied access, or unsupported item format produces a typed
backend-unavailable state. It never selects another storage path automatically.

### Linux adapter

Linux configuration must select one persistent host secret backend explicitly. Startup
must probe that backend for private ownership, durable atomic replace, exact-version
compare-and-swap, delete, and restart readback. Headless Linux must not assume that a
Secret Service session or desktop D-Bus exists.

The closed capability result is `ready` or `unavailable` with a stable reason such as
`not_configured`, `unsupported`, `locked`, `access_denied`, `integrity`, or `io`.
An unavailable backend disables enrollment, refresh, migration, and runner launch. It
does not activate an environment, plaintext-file, or legacy-pool fallback. Selection of
the first supported Linux backend is an implementation prerequisite for Linux product
acceptance, not a license to weaken this contract.

## Account Service

`decodexd` is the sole coordinator. It exposes versioned commands and bounded queries
for:

- device login and enrollment;
- explicit import from a daemon-readable, owner-private source descriptor;
- list and inspect;
- rename;
- enable and disable;
- logout and metadata deletion;
- proactive refresh and app-server callback refresh;
- explicit `Use in Codex`;
- usage, profile, history, quota, capability, and health refresh;
- one offline one-shot legacy account migration.

Every mutation carries a client command ID, idempotency key, and expected account
revision when an account exists. PostgreSQL stores the complete credential-negative
request identity, current operation phase, and exact public result. Exact replay returns
the stored result. Conflicting reuse fails before an effect.

List, inspect, usage, profile, history, and health responses contain no store locator,
token fragment, credential hash input, local secret path, or provider response body.

### Account-operation phases

Cross-store operations use one finite saga. They do not use a generic distributed
transaction coordinator. The durable phases are:

| Phase | Meaning |
| --- | --- |
| `prepared` | PostgreSQL committed the intent and fenced conflicting operations. No store effect is claimed. |
| `provider_effect_pending` | A refresh request can have reached the provider. This phase is used only when provider rotation can be ambiguous. |
| `store_applied` | Exact HostCredentialStore metadata proves the target version, fingerprint, provider binding, and writer operation. |
| `committed` | PostgreSQL committed the new account projection and final public receipt. |
| `cancelled` | No store change is accepted and the operation is terminal. |
| `recovery_required` | The system cannot prove a safe automatic continuation. The reason is typed and the account is ineligible. |

An unsettled operation fences another credential mutation for that account. Per-account
serialization and HostCredentialStore compare-and-swap provide the concurrency boundary.
The Account Service reconciles every nonterminal operation at startup before it admits
that account for routing.

### Enrollment and import

Enrollment first commits an Account UUID and `prepared` operation in PostgreSQL. Device
login may use one operation-scoped temporary Codex home. It must not become a runner home
or a persistent per-account home. The path is derived from the operation UUID under a
private Decodex temporary root and is removed after verified import or startup recovery.

Current Codex device login returns login metadata and writes credentials to its selected
auth backend. Before implementation relies on this flow, one narrow feasibility proof
must show that the supported exact Codex build can write device-login output to a
temporary, isolated, daemon-readable auth backend without changing ambient `~/.codex`.
If no supported method exists, device enrollment is typed unavailable. The implementation
must not assume that setting `CODEX_HOME` forces all current Codex auth storage into
`auth.json`.

Explicit import accepts a source descriptor, not credential bytes in the public protocol.
The daemon opens an owner-private local source without following links, validates one
supported format, derives the provider identity, and rejects a mismatch or duplicate.

For either path, the daemon validates the complete bundle, computes its target
fingerprint, creates the HostCredentialStore item only when absent, and then commits the
store metadata and ready account projection in PostgreSQL. Reconciliation applies these
rules:

- missing store item after `prepared`: cancel and require the user to repeat enrollment;
- exact item written by this operation: advance to `store_applied` and `committed`;
- different item or provider binding: enter `recovery_required` and keep the account
  ineligible;
- backend unavailable: retain the operation without routing and report the typed state.

Secret input that existed only in process memory is never reconstructed from PostgreSQL.

### Refresh and rotation

Proactive refresh and `account/chatgptAuthTokens/refresh` callbacks use one per-account
serialization boundary. A callback is bound to the ProcessGeneration Account UUID. It
cannot supply or select another account.

The Account Service reads one exact credential version, commits a `prepared` operation,
and moves it to `provider_effect_pending` immediately before the provider request. On a
successful response, it validates the provider identity and immediately performs one
compare-and-swap that stores all returned access, refresh, and identity-token changes as
one new bundle. It then records `store_applied` and commits the PostgreSQL projection.

A concurrent caller waits for the current operation. It then returns the new access token
for the same account or starts a new operation from the new version. A stale
compare-and-swap can never overwrite a newer refresh-token rotation.

After restart:

- an exact new store version written by the operation is committed to PostgreSQL;
- the unchanged old version before any provider request can be retried under a new
  operation;
- an unchanged old version with `provider_effect_pending` is provider-outcome ambiguous
  and is not replayed automatically unless the provider offers an accepted idempotent
  result-readback contract;
- provider ambiguity without such readback sets `reauth_required` and preserves one
  recoverable, fail-closed authority state.

This last result is recoverable by explicit enrollment. It does not claim that a process
crash can recover a rotated refresh token that the provider returned but no host store
durably accepted.

### Disable, logout, and delete

Disable is a PostgreSQL-only routing operation. It prevents new selection and launch but
does not terminate or rebind an existing process.

Logout first fences new launches. It fails with a typed `account_in_use` result while an
active ProcessGeneration or unsettled ProviderAttempt is bound to the account. The user
must disable the account and let existing work settle before retrying. There is no hidden
force-delete path.

After that check, logout commits `prepared`, deletes the exact credential version, and
commits the account as logged out and ineligible. Reconciliation treats a missing store
item as a completed delete, the expected old item as a safe delete retry, and any newer or
differently bound item as `recovery_required`.

Metadata delete is allowed only after logout. It creates a PostgreSQL tombstone and hides
the account from normal lists. Stable Account UUIDs, historical usage, operation receipts,
and execution references remain intact for audit and referential integrity.

### Runner launch and app-server projection

Routing selects an Account UUID only from PostgreSQL authority. Before spawn, the existing
ProcessGeneration intent binds the exact account revision, credential version, credential
fingerprint, provider binding, and Codex build. The Account Service reads that exact store
version and gives the credential bundle to the private Codex adapter.

The adapter projects the initial access token through the supported process-scoped
`account/login/start` `chatgptAuthTokens` request. Credentials do not enter process
arguments, a long-lived environment, public protocol, or logs. A refresh callback may
receive a newer access token only for the same bound account.

If the store is unavailable or its metadata differs from the ProcessGeneration intent,
launch stops before spawn. A crash after spawn follows the existing ProcessGeneration and
ProviderAttempt reconciliation contracts. The Account Service does not create a second
process or provider-effect ledger.

### Use in Codex

`Use in Codex` is an explicit user command. It projects one selected account into the
ambient current Codex authentication store through a supported, capability-probed Codex
adapter. It does not change Decodex routing order, sticky selection, or runner bindings.

The implementation must not treat direct `~/.codex/auth.json` replacement as a complete
current Codex contract because Codex can use Keychain or encrypted auth storage. A narrow
feasibility proof must identify a supported current adapter. If none exists, the command
returns `ambient_projection_unavailable` without changing ambient auth. Standalone Codex
and Decodex can otherwise use the shared home concurrently.

### Usage, profile, history, and quota

The Account Service reuses sound v0.2 parsing, refresh, selection, usage, profile, and
history domain logic after separating it from file storage. Provider responses are
normalized into credential-negative PostgreSQL projections.

Quota evidence preserves the exact window duration. The 300-minute and 10080-minute
windows are independent observations. The system does not infer them from primary or
secondary position and never pools, adds, averages, or transfers quota across accounts.
Routing exclusion and all-accounts-depleted waiting use the exact accepted windows and
their reset times.

## Readiness

Readiness has two separate checks:

- `CredentialStore` reports whether the configured host backend can perform durable
  read, compare-and-swap, and delete.
- `AccountLifecycle` reports whether the Account Service, PostgreSQL schema, provider
  adapter, startup reconciliation, and required store backend are complete.

An environment-only access-token projection is `projection_only`. It can never produce
`CredentialStore Ready` or `AccountLifecycle Ready`. The current
`EnvironmentCredentialVault` and legacy watcher therefore do not satisfy production
readiness. Any current `CredentialVault Ready` result from that implementation means
only that startup projection is present. It is not durable credential-lifecycle
readiness and must not satisfy a final gate. Production routing remains disabled while
either final check is unknown or unavailable.

## Offline one-shot migration

Legacy account migration is the only permitted v0.2 account-state ingress. It is an
explicit offline operation, not normal daemon startup.

The migration procedure:

1. Stops v0.2, `decodexd`, and every account helper.
2. Takes an exclusive migration lock.
3. Opens the exact owner-private legacy account file and mapping under their existing
   locks without following links.
4. Computes one exact source fingerprint and item count without logging content.
5. Preserves every established vNext Account UUID mapping and validates the provider
   identity bound to it.
6. Commits one idempotent PostgreSQL migration intent.
7. Creates or verifies each HostCredentialStore item with create/CAS semantics.
8. Verifies every destination version, fingerprint, provider binding, and PostgreSQL
   projection.
9. Commits one complete migration receipt.
10. Leaves the source bytes unchanged as cold backup.

Repeating the operation with the same source fingerprint returns or resumes the same
receipt. A changed source, changed mapping, conflicting destination, missing item, or
unavailable backend fails closed. No watcher, runtime fallback, environment projection,
compatibility API, or dual write remains after completion.

This migration imports account credentials and required account metadata only. It does
not scan or import Codex rollout history, Codex sessions, SQLite rows, Linear lanes,
runtime history, Goals, Projects, or Automations.

## Final clean cutover

The final installation must work after all of these paths are absent:

- the legacy `accounts.jsonl` watcher;
- the legacy UUID mapping bridge as a runtime input;
- access-token injection through the daemon environment;
- the legacy account helper and `:8192` server;
- direct SwiftUI or legacy CLI account-store access;
- dual account-control UI and runtime ownership.

The cold tag, untouched secret-bearing backup, redacted freeze inventory, and accepted
historical receipts remain evidence only.

## Implementation decomposition

Implementation follows this dependency order:

1. Prove the exact supported Codex enrollment and ambient-projection adapters. Implement
   the HostCredentialStore port, macOS adapter, Linux capability contract, and redaction.
2. Add PostgreSQL Account Registry lifecycle/operation authority and the daemon Account
   Service for list, inspect, rename, enable, disable, enrollment, import, logout, and
   startup reconciliation.
3. Add serialized proactive refresh, app-server callback refresh, CAS rotation, and
   exact runner credential projection.
4. Add usage, profile, history, separate quota ingestion, and complete versioned client
   protocol projections.
5. Add the offline one-shot migration, remove the watcher/environment/helper paths, and
   move GPUI, SwiftUI, and CLI to the common service.
6. Freeze the integrated boundary and run the high-value acceptance matrix once before
   routing enablement and whole-product cutover.

These are module and authority boundaries. They do not create separate daemons, homes,
event stores, transaction coordinators, or plugin frameworks.

## Deferred acceptance and fault matrix

| Boundary | Required evidence |
| --- | --- |
| Forced expiry and rotation | Expire one access token, refresh it, persist every returned token atomically, restart the daemon, and continue without legacy input. |
| Concurrent refresh | Race proactive refresh and multiple app-server callbacks for one account; prove one monotonic store version and no lost newer rotation. |
| Provider ambiguity | Crash or time out after refresh request but before a proven store write; prove no automatic unsafe replay and one typed `reauth_required` recovery path. |
| Store/PG partial failure | Inject failure before store write, after store write, and before PostgreSQL finalization for enrollment, import, refresh, and logout; prove deterministic reconciliation. |
| Backend unavailable | Lock or remove the configured backend; prove typed readiness, no fallback, no runner launch, and recovery after the same backend returns. |
| Active-run logout | Disable an account with active work, reject logout, settle the exact ProcessGeneration/ProviderAttempt, then complete one exact delete. |
| Runner launch | Bind one exact account and credential version, rotate the account during the run, and prove same-account callbacks cannot rebind the process. |
| Shared home | Create a Decodex thread, kill the process, rotate accounts, restart the daemon, and read the same thread in standalone Codex. |
| Ambient coexistence | Run standalone Codex and Decodex concurrently; prove routing does not mutate ambient auth and explicit `Use in Codex` is isolated and fail-closed. |
| Quota | Persist independent 300-minute and 10080-minute windows for multiple accounts; prove exclusion, fallback, and all-depleted wake without merged quota. |
| Credential absence | Inspect PostgreSQL, public protocol, logs, receipts, process arguments, crash output, and long-lived environment for credential material. |
| One-shot migration | Resume after each item boundary, verify stable vNext UUIDs and exact source fingerprint, preserve the cold source, and prove no second runtime ingress. |
| Final install | Start and use account operations and Reset Card with legacy account files, mapping watcher, helper, `:8192`, and dual UI removed. |

Any failure keeps production routing and final cutover disabled. It does not authorize a
legacy watcher or environment fallback.

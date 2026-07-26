# Account Lifecycle Authority

Status: normative vNext account authority for XY-1423. XY-1422 owns
implementation. This document defines the first usable macOS dogfood boundary and
the later complete account lifecycle. It does not claim that either boundary is
implemented.

The immediate target is `MacDogfoodReady`. Final `AccountLifecycleReady` has more
requirements. A component-first global gate is not the delivery order.

## Fixed boundaries

The account system has exactly three owners:

1. The PostgreSQL Account Registry owns credential-negative product state.
2. One HostCredentialStore owns versioned secret bundles.
3. The `decodexd` Account Service coordinates account operations.

Keep one daemon, one shared normal `~/.codex`, the same-UID typed protocol, exact
identifiers, PostgreSQL outbox and leases, and finite per-account compare-and-swap
operations. Credentials do not enter PostgreSQL, the public protocol, process
arguments, logs, or a long-lived daemon or child environment.

This boundary adds no event sourcing, generic distributed transaction coordinator,
new process or provider-effect ledger, per-account daemon, or permanent per-account
or per-run Codex home. V13 remains the repository-effect saga. V23 remains the
ProcessGeneration owner. V24 remains the ProviderAttempt owner.

One Codex process has one immutable Account UUID and provider binding for its
complete lifetime. A refresh callback can return a newer credential for that same
binding. It cannot select another account.

## Account Registry

The Account Registry owns:

- stable Account UUID, label, revision, tombstone, and provider identity;
- an administrative `enabled` boolean that is independent from observed state;
- observed account, authentication, capability, and health state;
- one versioned routing control with mode, fixed target, and complete account order;
- separate 300-minute and 10080-minute quota observations;
- current non-secret credential version, fingerprint, and provider binding;
- finite account-operation intents, phases, receipts, and reconciliation results;
- existing ProcessGeneration and ProviderAttempt references; and
- one-shot migration manifest identity and per-account results.

Observed state never encodes administrative enablement. Disabling an account does not
rewrite its last health or quota observation. Enabling an account does not make that
observation healthy. Eligibility requires both `enabled=true` and current positive
evidence for every applicable check.

PostgreSQL stores no credential, encrypted credential blob, retrieval locator, or
ambient Codex auth export. A fingerprint is equality evidence only.

## HostCredentialStore

The store has one record per Account UUID:

| Field | Contract |
| --- | --- |
| `schema_version` | Closed bundle format. |
| `account_id` | Exact Account UUID. |
| `provider_kind` | Closed provider kind, initially ChatGPT. |
| `provider_account_id` | Canonical provider identity. |
| `credential_version` | Positive monotonic compare-and-swap version. |
| `writer_operation_id` | Account-operation UUID that wrote this version. |
| tokens | Complete access, refresh, and optional identity token bundle plus required expiry/type metadata. |

Metadata readback returns only Account UUID, provider binding, credential version,
writer operation, and a domain-separated fingerprint of the canonical complete bundle.
Create-if-absent, exact-version compare-and-swap, and exact-version delete are atomic
for one Account UUID. A stale version, wrong provider binding, duplicate provider
identity, missing item, or unavailable backend is a typed result.

For macOS, the adapter uses non-synchronizing Keychain generic-password items under
application identity `box.acg.decodex` and service
`box.acg.decodex.credentials.v1`. The Keychain account name is the canonical Account
UUID. Accessibility is after first unlock, this device only. The daemon identity is
the only reader and writer. Locked, denied, malformed, or unsupported Keychain state
fails closed.

The Linux backend is a later `AccountLifecycleReady` obligation. It must be selected
explicitly and must prove persistent private storage, atomic replace, exact-version
compare-and-swap, delete, and restart readback. It has no environment or plaintext
fallback.

## Versioned account controls

Every mutation uses the same versioned protocol and supplies a client command ID,
idempotency key, and the applicable expected account or routing-control revision.
PostgreSQL stores the complete credential-negative request and exact public result.
Exact replay returns that result. Conflicting key reuse and stale revision fail before
mutation.

The commands are deterministic:

| Command | Compare-and-swap effect |
| --- | --- |
| `enable_account` | If the expected account revision is current, set `enabled=true`. Change only the account revision when the value changes. |
| `disable_account` | If the expected account revision is current, set `enabled=false`. Block new admission immediately. Do not terminate or rebind existing work. |
| `set_fixed_selection` | If the expected routing revision and target account revision are current, set mode `fixed` and the exact target Account UUID. Preserve account order. |
| `set_balanced_selection` | If the expected routing revision is current, set mode `balanced` and clear the fixed target. Preserve account order. |
| `set_account_order` | If the expected routing revision is current, replace the order with one complete duplicate-free permutation of all non-tombstoned Account UUIDs. Preserve mode and a valid fixed target. |

A fresh command whose desired value already matches returns a terminal no-change
receipt at the same revision. Any real change increments exactly one owning revision.
No command changes observed health, quota, credentials, ProcessGeneration, or
ProviderAttempt state as a side effect.

For a new task, `fixed` considers only its target. An ineligible fixed target returns a
typed no-route result. `balanced` selects the first fully eligible account in canonical
order after independent capability, credential, process, attempt, and two-window quota
checks. Manual recovery changes enablement, fixed/balanced mode, or order with these
commands and then submits a new task. It does not rebind or replay an existing thread.

Automatic cross-account same-thread fallback and all-depleted scheduler wake are not
Slice 1 requirements. An all-depleted result exposes the exact reset evidence and waits
for an explicit retry. V14 and V16 remain the policy/snapshot and decision authorities;
their broader automatic-routing behavior is accepted later.

## Account operations

Cross-store changes use one finite per-account operation journal:

| Phase | Meaning |
| --- | --- |
| `prepared` | PostgreSQL committed the intent and fenced conflicting account operations. |
| `provider_effect_pending` | A refresh request can have reached the provider. |
| `store_applied` | Exact store metadata proves the target version, fingerprint, binding, and writer. |
| `committed` | PostgreSQL committed the projection and public receipt. |
| `cancelled` | No store change is accepted and the operation is terminal. |
| `recovery_required` | Safe automatic continuation cannot be proved; the account is ineligible. |

An unsettled operation fences another credential mutation and new execution admission
for that account. Startup reconciles every nonterminal operation before the account can
be eligible.

Enrollment and explicit import commit `prepared` before a store write. Device login can
use one operation-scoped private temporary home. It is removed after verified import or
recovery and never becomes a runner home. Import accepts a daemon-opened owner-private
source descriptor, not credential bytes in the public protocol.

Refresh reads one exact credential version, records `provider_effect_pending` before
the provider call, validates the returned provider identity, and writes the complete
rotated bundle with one compare-and-swap. Concurrent callers serialize on that
operation. After restart, an exact store write can be committed. A provider request with
no proved store write is not replayed unless the provider has an accepted idempotent
result-readback contract. Otherwise, the account becomes `reauth_required`.

Logout disables new launch admission and rejects with `account_in_use` while an active
ProcessGeneration or unsettled ProviderAttempt is bound to the account. It then deletes
one exact store version through the same journal. Metadata deletion is allowed only
after logout and creates a tombstone. Historical UUIDs, receipts, and execution
references remain.

## Exact-build account capability

`AccountLifecycle` readiness for a build is positive evidence, not a schema assumption.
Before account-backed runner launch or a new Reset Card effect, the exact protected
Codex build must prove all of these facts:

- generated schema supports process-scoped `account/login/start` with
  `chatgptAuthTokens`;
- a live probe accepts that projection and reads back the same provider account;
- generated schema and a live callback transcript support
  `account/chatgptAuthTokens/refresh`;
- the Account Service can bind that callback to the exact ProcessGeneration, serialize
  refresh, complete credential compare-and-swap, and reply for the same provider binding;
- the exact build, schema fingerprint, and callback capability profile are cached and
  bound to launch authority.

Unsupported, unprobed, contradictory, or changed builds fail closed. The current vNext
adapter replies method-not-found to inbound app-server requests. That source behavior
does not service the refresh callback and therefore cannot satisfy `AccountLifecycle`,
`MacDogfoodReady`, or runner readiness. Initial token projection alone is insufficient.

The supported macOS build must also prove that device login can write to an isolated
daemon-readable auth backend without changing ambient `~/.codex`. Ambient `Use in
Codex` is a separate later capability and is not required for Mac dogfood.

## ProcessGeneration binding

The owning [ProcessGeneration authority](process-generation-authority.md) must extend
its existing V23 intent, launch-manifest identity, prepare command, and strict readback
with the canonical initial account revision, credential version, credential fingerprint,
provider binding, and exact-build account-capability profile. These fields are immutable
launch facts. Same-account callback rotation does not rewrite them.

Immediately before spawn, the Account Service must read the exact HostCredentialStore
metadata and compare every field with the ProcessGeneration intent and Account Registry.
Any mismatch stops before spawn. The existing ProcessGeneration and ProviderAttempt
state machines own crash and effect ambiguity. No account-specific process or effect
ledger is added.

## Reset Card fencing

Reset Card keeps its existing exact provider-credit ID, provider key, durable receipt,
and authoritative readback. New admission and the final pre-effect fence both require:

- the exact account revision and `enabled=true`;
- `AccountLifecycle=ready` for the active platform and exact Codex build;
- no unsettled account operation other than reconciliation of this exact receipt;
- exact Account Registry and HostCredentialStore credential version, fingerprint, and
  provider-binding agreement; and
- the existing admissible observed state and exact public card descriptor.

The final fence repeats these checks in the effect-start transaction. A disable,
operation start, revision change, or store drift between discovery and effect prevents
the provider call.

Receipt handling is ordered differently from new admission. After same-UID transport
and exact request-fingerprint checks, a durable terminal receipt replays unconditionally
before current enabled, readiness, health, operation, store, or revision gates. A
terminal receipt never calls Codex or the provider again. Nonterminal status and required
reconciliation also remain readable after a gate changes. They cannot start a new effect.

## One-shot migration manifest

Legacy account migration is one explicit offline operation. Normal daemon startup never
reads a legacy account file, mapping, helper, or environment projection.

The operation first creates canonical
`decodex/account-migration-manifest/1`. The manifest lists every source that contributes
one output field. Each source entry has a closed role, present-or-absent state, private
path identity, exact byte count, and SHA-256 of the unchanged bytes. Required roles and
default current paths are:

- legacy credential/provider records, disabled flags, and physical order at
  `~/.codex/decodex/accounts.jsonl`;
- legacy label offsets and fixed selector at `~/.codex/decodex/config.toml`;
- the established vNext UUID bridge at
  `~/.decodex/reset-card-legacy-map.json`, when present; and
- established vNext Account UUIDs and display labels at `~/.decodex/config.toml`, when
  present.

No live `:8192` response is migration authority. An additional source is rejected unless
its role and fingerprint are part of the same manifest. The canonical manifest digest
covers the sorted source entries and all normalized output below.

Each normalized account entry contains the source ordinal, target Account UUID,
provider kind and identity, label, enabled value, target credential version, and target
provider binding. The routing entry contains exactly one mode (`fixed` or `balanced`),
an optional fixed target UUID, and the complete ordered Account UUID list. A legacy
selector must resolve exactly once. Legacy `disabled=true` becomes `enabled=false`; an
absent or false flag becomes `enabled=true`. Physical account-record order is the default
account order. A present selector makes the mode `fixed`; absence makes it `balanced`.
An established vNext UUID or label must agree with the bridge and source identity. Label
precedence is established vNext display label, then the v0.2 provider-identity/offset
derivation, then deterministic `Account NN`. An absent destination starts at credential
version 1. A verified exact existing destination retains its positive version; any other
existing destination is a conflict.

The import policies are explicit and fixed:

| Data | Policy |
| --- | --- |
| credentials and provider identity | Import to the HostCredentialStore and verify exact metadata. |
| labels, enabled state, mode, and order | Import from the normalized manifest. |
| 300-minute and 10080-minute quota | Reset each window to `unknown` with no imported observation. |
| usage and profile projection | Start empty and obtain fresh provider observations later. |
| account, Codex thread, and execution history | Do not import. |

The Account Service commits one idempotent migration intent for the manifest digest,
creates or verifies each store item, verifies every PostgreSQL projection, and commits
one credential-negative receipt. The same digest resumes or replays. Source, mapping,
destination, provider, or policy drift fails closed. Source bytes remain untouched as
cold evidence.

## Readiness levels

| Obligation | `MacDogfoodReady` | Final `AccountLifecycleReady` |
| --- | --- | --- |
| Host secret backend | macOS Keychain adapter accepted | macOS plus an explicitly selected persistent Linux backend |
| Exact-build auth | Initial projection and refresh callback proved for each accepted macOS build | Proved for every supported platform/build |
| Account lifecycle | Enrollment/import, list/rename, enable/disable, logout, refresh/CAS, startup reconciliation, and migration | Same contract across all supported hosts plus full fault acceptance |
| Routing | Initial eligible quota-aware fixed/balanced selection and explicit manual recovery | Automatic same-thread fallback and all-depleted wake after their later gate |
| Presentation | Minimal Accounts, Conversation, and Health data | Full bounded usage, profile, and history presentation |
| Ambient Codex auth | Deferred; no `Use in Codex` requirement | Capability-probed `Use in Codex`, fail-closed when unsupported |
| Legacy authority | No watcher, helper, or credential environment input on normal startup | Same, across every supported installation |
| Evidence | Two-account Mac flow with restart boundaries and package proof | Broader platform and adversarial matrix |

`CredentialStore` reports backend capability. `AccountLifecycle` reports the Account
Service, PostgreSQL authority, provider adapter, exact-build account capability, startup
reconciliation, and active host store. An environment-only projection is
`projection_only` and cannot satisfy either readiness result.

## Later obligations

The later readiness table in the [vNext gate manifest](vnext-gates.md) retains Linux,
ambient `Use in Codex`, full account presentation, automatic fallback and wake,
retained-title Desktop discovery, broad matrices, graph, automation, remote access, and
product polish. These obligations do not block the three Mac delivery slices unless a
slice explicitly names them.

Accepted historical receipts remain historical. A failure in this contract keeps the
affected account or readiness boundary unavailable. It does not restore a watcher,
environment projection, helper, dual write, or compatibility API.

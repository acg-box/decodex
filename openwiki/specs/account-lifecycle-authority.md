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

For macOS, the adapter uses non-synchronizing Keychain generic-password items with
item label `box.acg.decodex` and service namespace
`box.acg.decodex.credentials.v1`. The Keychain account name is the canonical Account
UUID. Accessibility is after first unlock, this device only. The daemon identity
defined below is the only reader and writer. Locked, denied, malformed, or unsupported
Keychain state fails closed.

The daemon runs only as one no-UI app-like wrapper with bundle identifier
`box.acg.decodex.daemon` and main executable `Contents/MacOS/decodexd`. The selected
local dogfood identity is provisioned team `T54QFA7W2S`, application identifier
`T54QFA7W2S.box.acg.decodex.daemon`, and profile channel `development`. That
application identifier is the daemon's sole effective Keychain access group. Every
Keychain read, create, compare-and-swap, delete, and metadata query sets that exact
group. Metadata verification includes the returned `agrp` attribute. A raw workspace
binary, a raw helper in the outer app, or `~/.local/bin/decodexd` can be a build input
or retired artifact. It is never an Account Lifecycle execution entry.

The wrapper has one fixed `Info.plist`, a valid hardened-runtime signature, an embedded
provisioning profile, and exact signed entitlements. The profile, signature, and
entitlements must agree on bundle identifier, application identifier, team, and the
single effective access group. The signed entitlement and access-group sets are closed:
missing, extra, or duplicate values refuse. This checkpoint accepts only the
`development` profile channel. It is not public distribution or notarization evidence.
The wrapper composer and verifier are deterministic and daemon-specific. They do not
accept arbitrary identities, profiles, entitlements, groups, channels, or fallback
binaries. SwiftUI and the CLI remain clients and receive no Keychain authority.

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

### Source-path security boundary

The one-shot migration source must remain below the real login home from
`pwd.getpwuid(euid).pw_dir`, where `euid` is the process effective UID. The canonical
gate must not read or mutate live default sources under `~/.codex/decodex`. It supplies
explicit operator-provided source paths in one run-unique, gate-owned fixture subtree
below that real login home. The fixture subtree has exact mode 0700 and every source
path in it must pass the same predicate below. This fixture location selects gate data;
it does not replace the login-home authority. Ambient `HOME`, a synthetic passwd
record, a different login-home or root authority, and a weaker fallback predicate are
not authority.

Each present source file and each generated credential file must be opened without
following a symbolic link. It must be a regular file that is owned by the effective
UID, has one link, and has exact mode 0600. Each direct source or secret-bearing parent
and the generated credential directory must be reached without following a symbolic
link. It must be a directory that is owned by the effective UID and has exact mode
0700.

Every ancestor above that direct private boundary must be a directory reached without
symbolic-link traversal. An ancestor can be owned only by the effective UID or by root.
In both cases, group and other write bits must be clear (`mode & 022 == 0`). Reject an
ancestor that has another owner. Read or execute bits for group or other do not make an
ancestor unsafe. Thus, an effective-UID-owned ancestor with mode 0750 or 0755 can pass,
while the direct private boundary and files remain 0700 and 0600.

The installer and each Rust migration child must enforce the same predicate. They must
not change the mode of the real login home to make migration pass. They must not create
a weaker compatibility path.

This is a POSIX path predicate. Owner, type, link, mode, and no-follow checks do not
prove that arbitrary ACL entries are absent. ACL semantics remain an explicit residual
and non-regression boundary. This amendment does not authorize an ACL change, and an
acceptance record must not claim that modes 0700 and 0600 prove every ACL property.

This path-policy amendment does not change `ExistingHydrate`, `AbsentInitialize`,
operation-first replay, the exact revision sequences, continuous installer lock
lineage, child capability limits, receipt ordering, or the no-new-ledger and
no-new-public-API decisions.

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

### Migration transition

The normalized manifest defines the final desired account administration. It does not
define the current-state precondition for an existing-row credential import.

The normalized manifest freezes each account's Account UUID, operation ID, provider
and target binding, and desired administration. When migration requires a credential
mutation, the finite account-operation descriptor is persisted atomically at
preparation and becomes the transition identity. It has one of these private typed
forms:

| Transition | Required behavior |
| --- | --- |
| `AbsentInitialize { expected_revision: None }` | PostgreSQL has no non-tombstoned Account UUID and the HostCredentialStore has no item. The existing finite import operation uses the manifest label and enabled value as initial administration. The exact target starts at credential version 1. |
| `ExistingHydrate { revision, display_label, enabled }` | PostgreSQL already has the Account UUID. For a credential-empty account, the complete current tuple is the existing-row credential-import precondition. After the exact credential binding is durable in the HostCredentialStore and PostgreSQL, the executor applies the manifest label and enabled value through the existing exact administration operation. An already exact positive target is verified without a credential mutation; another positive binding is a conflict. |

These names describe a private migration plan. They are not public protocol types,
PostgreSQL operation kinds, durable migration states, or a fourth authority owner.
`account_migration.rs` owns transition selection, final administration, routing,
receipt sequencing, and destination verification. `account_service.rs` maps the plan
to the existing finite credential operation.

After the manifest is frozen and before the first destination or credential mutation,
the Account Service uses one narrow cutover entry point. On the same single-use
PostgreSQL migration connection, it creates a session-local typed handoff that contains
exactly the manifest SHA-256 digest, canonical manifest JSON, and account count. V27
creates the receipt authority before its first account mutation, consumes the exact
handoff, and commits the prepared singleton receipt and all V27 effects in one
transaction. A populated V26 database without the exact handoff fails before mutation.
An empty or fresh migration can omit the handoff; if one is present, V27 consumes it
exactly. The handoff is not durable authority and ends with the connection. After V27,
`prepare_migration_intent` is exact no-insert readback. An absent receipt remains
absent and fails closed.

The singleton manifest intent freezes the exact migration target: Account UUID,
provider binding, positive credential version, writer operation ID, credential
fingerprint, and the exact PostgreSQL/HostCredentialStore binding. A verified existing
positive credential binding can continue only when it equals this target. Same-digest
resume or replay accepts only this target. A different provider, version, writer,
fingerprint, store binding, account tuple, or manifest digest fails closed. Migration
never overwrites or downgrades another valid positive credential binding.

The normalized manifest contains one complete non-secret daemon-wrapper descriptor.
It contains the fixed wrapper and executable paths, executable digest and byte count,
the raw `Info.plist` digest and fixed bundle/executable fields, the raw
embedded-profile digest, application identifier, team, expiry, and closed channel,
the canonical complete signed-entitlement digest, the exact normalized access-group
set, and the canonical signature-identity digest. These facts are part of the existing
canonical manifest digest. They are not decision fingerprints.

The completed retirement receipt stores `daemon_wrapper_verified=true` and
`daemon_wrapper_identity_sha256`. That value must equal the canonical digest of the
exact `daemon_wrapper` descriptor in the manifest. LaunchAgent
`ProgramArguments[0]` must equal the descriptor's wrapper main. The existing
installed-asset set must contain exactly one matching executable path, digest, and byte
count. `Info.plist` and the profile remain descriptor fields, not executable installed
assets.

The installer validates current wrapper identity before every migration, prepared,
finalizer, and completed-verifier child. The corresponding Rust child runs the same
fixed inspector at the initial, prepared, final, or completed boundary and compares its
current identity with the manifest or completed PostgreSQL receipt. Initial migration
checks precede the V27 handoff, PostgreSQL mutation, and Keychain effect. Prepared
verification checks precede staging or legacy retirement. Finalization checks precede
the completed receipt. Completed verification obtains the frozen manifest from
PostgreSQL and does not depend on the retired source manifest. After the final or
completed child returns, the installer alone performs one more current-identity check
immediately before the launch decision. Any drift, including profile expiry after
preparation, fails closed without rebinding the manifest or adding an operation,
revision, or receipt. This extends the existing manifest and receipt; it does not add a
ledger, receipt family, or authority owner.

#### Cutover lock capability

The macOS installer is the one physical cutover coordinator and the normal operator
entry point. Before it spawns a migration child or changes any destination,
credential, configuration, or retirement state, it opens and verifies
`server/decodex.lock` and acquires the same exclusive nonblocking `flock` as the local
listener. It verifies the same no-follow server-directory and lock-path type,
configured owner, mode, link count, device, and inode authority.

The installer owns one RAII guard for its original descriptor and locked open file
description. It does not unlock, close, replace, or unlink that authority until the
final launch decision or failure cleanup. It retains the guard through migration-child
completion, its own final-configuration swap, exact staging and active-legacy
retirement, finalizer or completed-verifier children, and completed receipt. This uses
the existing `decodex.lock`; migration must not create another lock name or a separate
migration lock. The installer releases its guard last on success or after exact
failure cleanup.

For each `decodexd` migration, prepared-verifier, finalizer, or completed-verifier
spawn, the installer passes one `dup`-derived borrowed descriptor through an explicit
installer-only inherited-FD capability and retains its original guard. The parent-side
duplicate remains non-inheritable and close-on-exec. The exact
`Popen(pass_fds=...)` call transfers it only for that spawn, and the parent closes its
duplicate after the spawn. The child validates only facts that its descriptor and
filesystem view can establish:

- the inherited descriptor is open and refers to a regular file with the exact device,
  inode, configured owner, mode, and link count;
- the current server-directory path passes the required no-follow directory identity
  and metadata checks;
- the current `decodex.lock` path is reached without following links and is a regular
  file whose identity and metadata match the inherited descriptor; and
- it is immediately marked close-on-exec for all child descendants.

The child does not call `flock`, open a contention probe, or claim that it can prove
the installer identity, the current lock state, or whether an otherwise identical
descriptor came from `dup` instead of an independent open. It never unlocks, unlinks,
or replaces the lock. It closes only its borrowed descriptor on exit and cannot close
the installer's descriptor.

Resource ownership starts immediately after the first successful lock acquisition,
descriptor duplication, or socket creation. The production child runner and the
canonical gate conditionally close every acquired lock duplicate, transition-gate
duplicate, socket, pipe, and child process when a later duplication, spawn, identity
capture, or checkpoint fails. Cleanup does not replace the primary failure. A cleanup
failure is retained as an explicit secondary failure. Fault cases must prove that a
new installer can acquire the exact lock after cleanup. This rule uses the existing
guard and adds no lock or generic resource framework.

A missing FD, invalid descriptor, wrong file type or metadata, pathname mismatch, or
identity drift returns a typed operator refusal before PostgreSQL, Keychain,
configuration, or retirement effects. Direct ordinary invocation without the explicit
borrowed-FD shape also refuses before effects. Migration, prepared verification,
finalization, and completed verification are hidden installer commands, not supported
standalone authority surfaces.

Physical continuity proof belongs to the installer coordinator's source ownership and
the external canonical gate, not to child self-attestation. Within this cooperative
same-UID boundary, installer construction establishes that its borrowed descriptors
refer to the retained open file description. An intentional same-UID caller can
imitate the capability shape; this is outside the confinement claim. The capability
is not a public protocol, durable token, generic lock framework, cryptographic parent
identity, or hostile same-UID defense.

The flock remains held while any descriptor for the locked open file description
survives. Child death leaves the installer's original guard held. Abrupt installer
death leaves the flock held while a child duplicate survives. The flock releases only
after the final such descriptor closes. Only then can another installer acquire the
verified lock for exact same-digest resume.

#### Operation-first replay

After singleton manifest intent and target validation, migration reads the exact
manifest operation ID before it classifies current account or store state. If that
operation exists, its immutable account, `Import` kind, provider binding, target
binding, and operation identity must equal the manifest-owned facts. Its persisted
expected revision, requested label, and requested enabled value must form one valid
`AbsentInitialize` or `ExistingHydrate` descriptor. That descriptor remains the
transition identity. Migration then resumes or reconciles the operation from its
persisted phase. A descriptor or target difference fails closed.

State-based `AbsentInitialize` or `ExistingHydrate` classification occurs only when
the manifest operation ID does not exist. Preparation atomically persists the selected
descriptor and applies the existing PostgreSQL equality and revision checks. Another
unsettled operation for the account is a conflict. An `AbsentInitialize` operation
that already created its PostgreSQL row remains
`AbsentInitialize { expected_revision: None }` after restart. The new row does not
reclassify it as `ExistingHydrate`.

Migration-aware recovery owns manifest-bound `Import` phases after lock, intent, and
target validation. Generic startup reconciliation must defer such an operation. It
must not cancel a manifest-bound import in `prepared` or `recovery_required` before
migration can create or verify the exact Keychain item. This exception does not apply
to another account operation or to normal runtime recovery.

The allowed account-revision sequence is:

| Transition | Exact revision sequence |
| --- | --- |
| `AbsentInitialize` | Before preparation, no row exists. Preparation creates revision 1. `prepared` and `store_applied` remain at 1. Credential commit produces revision 2. Desired administration is an exact no-change at 2 because initialization persisted the same manifest label and enabled value. A different persisted initialization is an identity conflict, not an update to revision 3. |
| Credential-empty `ExistingHydrate` | The account starts at revision `r`. `prepared` and `store_applied` remain at `r`. Credential commit produces `r+1`. Exact administration remains at `r+1` when the manifest label and enabled value already match, or produces `r+2` when either desired value changes. |

In particular, a populated V26 account that V27 initializes with `enabled=false`
hydrates with current `false` and then applies a normal manifest's desired `true`. It
must not pass desired `true` as the existing-row import precondition. Routing has its
own revision and changes only after all final account projections. Same-digest replay
adds no account or routing revision.

A matching `committed` operation resumes at final administration and verification. A
matching `prepared` or `store_applied` operation resumes from that exact phase. A
matching manifest-bound `recovery_required` Import remains owned by migration-aware
recovery after the same manifest, operation, transition descriptor, provider, target
binding, and credential source are validated under the installer and account locks.
Migration first reads the exact target from the HostCredentialStore. An exact target
continues the existing operation. `NotFound` permits one create-if-absent attempt with
the same source bundle and target binding. Create success or `AlreadyExists` followed
by exact target readback moves the existing operation through `store_applied` to
`committed`. Unavailable, mismatched, corrupt, or ambiguous store state remains
`recovery_required` and returns typed not-ready. Cancellation of a manifest-bound
Import is forbidden in both `prepared` and `recovery_required`. An already `cancelled`
or identity-conflicting operation also refuses. Migration does not invent another
operation ID, reset the phase, reclassify the transition, or broaden normal runtime
recovery.

The ordered transition is:

1. The installer acquires and verifies the existing namespace lock capability.
2. The migration child validates its borrowed capability.
3. V27 consumes the session-local exact handoff and commits or replays the singleton
   manifest intent with its account-cutover effects.
4. Read and resume the exact manifest operation, or classify state only when it does
   not exist.
5. Complete or reconcile the existing finite credential operation.
6. Apply manifest label and enabled state through exact administration.
7. Replace routing only after every final account projection is verified.
8. Verify the complete PostgreSQL and HostCredentialStore destination.
9. On prepared resume, the prepared verifier validates its borrowed capability,
   current wrapper identity, frozen manifest, and complete destination before
   retirement continues.
10. The installer swaps final configuration when required and retires staging secrets
    and active legacy authority while it retains the lock.
11. The finalizer validates its borrowed capability and current wrapper identity, then
    commits the completed destination and retirement receipt.
12. On completed replay, the completed verifier validates its borrowed capability,
    current wrapper identity, frozen PostgreSQL manifest, receipt, and current
    destination without reading retired sources.
13. After the final or completed child returns, the installer verifies current wrapper
    identity immediately before it decides whether to launch, then releases its lock
    last.

Restart recovery preserves this order across the manifest intent, operation
`prepared` before Keychain create, Keychain create, `store_applied`, PostgreSQL
credential commit, administration, routing, retirement, and final-receipt
checkpoints. Exact same-digest replay must not add a credential write,
administration revision, routing revision, or second receipt.

Completed-state verification compares current state with the completed destination
receipt. It checks label, enabled state, routing mode and complete order, provider
binding, credential version, writer operation ID, fingerprint, and exact
HostCredentialStore/PostgreSQL binding. Drift fails closed before daemon launch.

Migration must not enable an existing account before exact credential binding, relax
existing-row equality or revision checks, change the V27 fail-closed
`enabled=false` backfill, add a migration ledger or migration-specific durable
operation kind, add fallback authority, or expand into SwiftUI, GPUI, Quick Task, or
remote behavior.

The import policies are explicit and fixed:

| Data | Policy |
| --- | --- |
| credentials and provider identity | Import to the HostCredentialStore and verify exact metadata. |
| labels, enabled state, mode, and order | Import from the normalized manifest. |
| 300-minute and 10080-minute quota | Reset each window to `unknown` with no imported observation. |
| usage and profile projection | Start empty and obtain fresh provider observations later. |
| account, Codex thread, and execution history | Do not import. |

The Account Service supplies one session-local manifest handoff to V27, creates or
verifies each store item, verifies every PostgreSQL projection, and completes one
credential-negative receipt. The same digest resumes or replays. Source, mapping,
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

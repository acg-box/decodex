# OpenWiki Quickstart

Decodex is resetting its vNext PostgreSQL architecture before deployment. There are no
external or deployed users, and current local PostgreSQL data is disposable development
state. The accepted target has one canonical unversioned latest schema, no database
migration system, and runtime-only daemon startup.

This is target authority, not a claim about current source. Existing numbered SQL,
Refinery integration, schema-history checks, upgrade tests, and the executable storage
spike are implementation drift to remove. Historical files that describe those paths are
superseded provenance only.

OpenWiki is the repository knowledge entrypoint. Source, tests, the one latest schema,
configuration, and accepted contracts remain executable authority.

## Start here

- [vNext authority decision](decisions/vnext-authority.md): the accepted no-migration
  reset, Candidate-5 continuity, product shape, and supersession boundary.
- [vNext authority contract](specs/vnext-authority.md): normative entities, PostgreSQL
  schema ownership, runtime boundaries, Candidate-5 Quick Task, account continuity, and
  delivery contract.
- [vNext gate manifest](specs/vnext-gates.md): source-freeze, latest-schema, runtime,
  Candidate-5, account, and local cutover gates.
- [Runtime architecture](architecture/runtime-architecture.md): process topology,
  PostgreSQL bootstrap/runtime separation, independent startup readiness, service
  ownership, and current-state drift.
- [Commands and validation](operations/commands-and-validation.md): exact hidden bootstrap,
  current-authority validation, and local account restore commands plus repository checks.
- [Account lifecycle authority](specs/account-lifecycle-authority.md): Account Registry,
  HostCredentialStore, Account Service, current account observations, and the local
  credential-negative rebind.
- [ProcessGeneration authority](specs/process-generation-authority.md): durable pre-spawn
  fencing, exact process identity, positive-only death evidence, and account-local
  quarantine.
- [ProviderAttempt authority](specs/provider-attempt-authority.md): one external-turn
  attempt owner, positive-only outcome evidence, and replay prohibition.
- [Stateless execution coordination](specs/execution-coordinator-authority.md): closed
  Conversation/ManagedRun consumer integration and Candidate-5 owner sequencing.
- [Design rationale](decisions/design-rationale.md): retained product design choices.
- [Runtime operator workflows](workflows/runtime-operator-workflows.md): current operator
  workflows; treat any schema-upgrade text there as stale until its owner aligns it.
- [Private-artifact archive](specs/private-artifact/README.md): historical evidence only.
- [v0.2 freeze receipt](evidence/v0.2-freeze.md): frozen legacy provenance only.

## Schema authority

`crates/decodex-postgres` owns exactly one executable schema source:
`crates/decodex-postgres/schema.sql`. It contains final object definitions directly. It
does not contain an ordered history, upgrade branch, compatibility DDL, drain, backfill,
or old-state cleanup.

One explicit operator bootstrap runs only against an empty PostgreSQL 18 target. It:

1. resolves the schema-owner credential;
2. proves that the target is clean;
3. executes the complete latest schema in one transaction;
4. verifies the exact resulting catalog and configured authority; and
5. commits only after every check passes.

A second bootstrap against that nonempty target fails closed. The exact hidden command
is `decodexd bootstrap-latest-schema`; it is not `decodexd` startup. The exact read-only
validation command is `decodexd validate-current-authority`.

Normal `decodexd` startup resolves only the runtime database credential. It executes zero
DDL and verifies the exact current catalog and authority. It never resolves a schema-owner
credential, runs an upgrade, repairs a catalog, or consults a schema-history relation.

The reset preserves PostgreSQL 18, data checksums, `pgcrypto`, verified Unix-socket and
peer-UID checks, full schema/function/trigger/dependency/ownership/ACL/semantic
attestation, and negative PUBLIC/runtime checks. It removes history and upgrade
predicates only.

Exact-command receipts, account operations, ProcessGeneration, ProviderAttempt,
`schema_fingerprint`, runtime authority, activity, outbox, and repository-effect evidence
remain domain integrity records. They are not schema migration records.

## Repository map

- `crates/decodex-core/` owns domain and application contracts, typed `~/.decodex`
  paths/configuration, content-addressed blob contracts, and disposable cache contracts.
- `crates/decodex-protocol/` owns the exact-current owner-only same-UID Unix WebSocket
  protocol used by clients.
- `crates/decodex-postgres/` owns `schema.sql`, empty-target transactional bootstrap,
  current catalog/authority verification, PostgreSQL adapters, exact commands, leases,
  activity/outbox, account state, routing state, Conversation history, RuntimeSession,
  managed repositories, ProcessGeneration, and ProviderAttempt persistence.
- `crates/decodex-codex/` owns typed app-server contracts, exact-build capability
  profiles, redaction, and one-account-per-process adapter behavior.
- `crates/decodex-runtime/` owns `decodexd` service assembly and is the only library owner
  that composes infrastructure adapters. It records independent immutable startup
  projections for ProductStore, Quick Task, and ManagedRepository without adding a
  capability manager.
- `apps/decodexd/` is the sole server composition root. Its normal serve path is
  runtime-only.
- `apps/decodex-cli/` and `apps/decodex-gpui/` are protocol clients. They do not read
  PostgreSQL, credentials, rollout files, blobs, or repositories directly.
- `apps/decodex-app/` is a credential-negative native account client. It has no local
  account pool, helper/server authority, or daemon lifecycle owner.
- `apps/decodex/` is frozen v0.2 provenance outside the active Cargo workspace.
- `spikes/vnext-storage/` is retired as an executable schema owner. Historical feasibility
  evidence may remain, but no product or validation path may execute its schema.
- `site/`, Radar, Publisher, plugins, and automations remain separate auxiliary surfaces.

## Runtime summary

`decodexd` serves the exact-current protocol at the fixed owner-only
`~/.decodex/server/decodex.sock` endpoint. Both sides verify kernel peer UID and stable
server identity. Remote and cross-UID control remain disabled until their security gate.

One daemon and one endpoint remain available when Quick Task execution or
ManagedRepository is unavailable. `ProductStore` means verified PostgreSQL only. Quick
Task and ManagedRepository startup cannot overwrite it. Protocol and doctor report all
three readiness results separately.

When PostgreSQL is available, startup verifies the pinned socket, PostgreSQL 18 and data
checksums, `pgcrypto`, the exact latest catalog, closed dependencies, object ownership,
function and trigger bodies/settings/bindings, ACLs, configured runtime authority, and
negative PUBLIC/runtime conditions. Any drift produces typed unavailability. Startup
does not mutate the database.

The runtime identity has only the exact relation, sequence, and function authority needed
by current adapters. It cannot own Decodex objects, execute DDL, bypass triggers or
retention, use grant options, reach schema-owner authority through role membership, or
control a Decodex extension member. Hostile `search_path`, overload, trigger, rule,
policy, RLS, external-cascade, and default-ACL paths fail closed.

`decodexd` starts the account observation service independently from client queries. It
refreshes different accounts concurrently. Within one account it settles Reset Card work
before profile observation, coalesces successor rounds, and publishes only to a matching
account revision/cache generation. Account and Reset Card/profile queries read the
daemon-owned cache or persisted projection without contacting the provider, joining a
refresh, or starting refresh work. Candidate 5 must preserve this current-main behavior.

ProcessSupervisor projects restored nonterminal ProcessGenerations to `death_unknown`
and performs positive-only reconciliation. Uncertainty quarantines only the affected
account. ProviderAttemptService projects prepared or dispatch-authorized attempts to
`unknown` after restore and performs positive-only reconciliation. Process death, EOF,
timeout, restart, absence, or negative search never proves provider non-submission.

ExecutionCoordinator is crate-private and stateless. It sequences accepted route,
continuation, process, and provider-attempt owners. It stores no receipt or lifecycle and
cannot authorize dispatch by itself.

After fallible owners validate their dependencies, `QuickTaskRuntime` construction is
infallible and performs no I/O. Composition records one immutable ready or typed
unavailable result. Quick Task commands repeat all current owner fences. ManagedRepository
is independently `Ready`, `Disabled`, or typed `Unavailable`; repository failure affects
repository operations only.

## Candidate-5 Quick Task

Candidate 5 remains the product target and is not yet accepted implementation. It is one
ordinary multi-turn Conversation. The initial authority order is:

```text
Conversation creation
-> prospective Turn intent
-> complete routing snapshot
-> one route decision
-> first snapshots + starting RuntimeSession + inert initial plan
-> atomic first Turn + first Message admission
-> selected-account pre-spawn fence
-> fresh ProcessGeneration
-> exact RuntimeSession thread start and bind
-> ProviderAttempt
```

Routing Decision is the sole account selector. Account Service fences only that selected
account immediately before spawn. Initial planning has no source RuntimeSession and no
sticky member. It creates the first session; it is not same-thread reuse, explicit
successor, or Context Pack fallback.

The final latest schema contains both closed routing lineage shapes: all six source
lineage fields absent for the initial operation (`L0`), or all six present with positive
revisions for existing-session work (`L6`). Conversation work permits `L0` or `L6`.
ManagedRun work permits only `L6`. The latest schema creates the final constraints,
functions, and trigger bindings directly.

Every process, thread, and provider fence locks the exact selected active revision-1
Turn. Only a fresh ProcessGeneration result can spawn. Replayed, rejected, or uncertain
state cannot spawn, substitute an account, create a successor, duplicate an attempt, or
terminalize the Turn. Only positive definite pre-effect refusal can let Conversation
authority move that Turn to failed revision 2.

Immediately before spawn, runtime validates the selected working directory by no-follow
descriptor traversal, exact identity, directory type, ownership by the daemon effective
UID, and accepted path policy. Ambient current directory and repository discovery are not
authority. One broken repository cannot disable unrelated Quick Tasks.

Automatic cross-account same-thread fallback and all-depleted wake remain disabled under
XY-1304.

## Local database reset

After the operator stops the daemon and bootstraps the replacement database, the exact
hidden `decodexd restore-local-account-authority --root ROOT --schema-owner-user USER
[--schema-owner-credential-env-var ENV]` command reads one strict
`decodex/local-account-authority-restore/1` JSON document from stdin. It acquires and
retains the existing same-UID local transport namespace, proves every exact Keychain
binding before PostgreSQL mutation and again before commit, and accepts only a fresh
latest-schema target. It restores only current account rows, account order, and routing
control, then proves the complete tuple by exact readback.

For `fixed` mode, the fixed target is non-null and belongs to the retained account set
and order. For `balanced` mode, it is null. The order remains a complete duplicate-free
permutation of retained accounts whether each account is enabled or disabled.

Credential agreement may invoke only the existing `HostCredentialStore` owner. That
owner performs a confined in-process exact read, recomputes and compares the credential
fingerprint and binding, and returns only a typed credential-negative agreement result.
The operator action and its result never expose, serialize, copy, log, persist, rotate,
delete, or return token bytes. The action creates no public product or migration API,
generic attestation framework, metadata sidecar, backup/rollback path,
receipt/finalizer, or fallback. Normal startup and installation do not read an old
database or account source.

## First commands

Current repository discovery and source-validation entrypoints include:

```sh
cargo run -p decodexd -- --version
cargo run -p decodex-cli -- status
cargo run -p decodex-cli -- doctor --output json
cargo run -p decodex-cli -- account list
cargo run -p decodex-gpui
cargo test -p decodex-core --all-targets --all-features
cargo make test-vnext-architecture
cargo make check
```

Do not use historical schema-provisioning or migration commands as acceptance for this
reset. The implemented hidden commands are `decodexd bootstrap-latest-schema`,
`decodexd validate-current-authority`, and
`decodexd restore-local-account-authority`. See
[Commands and validation](operations/commands-and-validation.md).

## Safety rules

- Do not read `.env` files or secret-bearing live configuration.
- Do not route vNext product state through frozen v0.2 SQLite, Linear lanes, the old
  account watcher, an environment token projection, helper/`:8192`, or a compatibility
  fallback.
- Do not add numbered SQL, Refinery, a schema ledger, a schema generator, or a second
  executable schema owner.
- Do not make daemon startup resolve schema-owner credentials or execute DDL.
- Do not make Quick Task or ManagedRepository startup failure fatal to an otherwise usable
  daemon. Do not hide it through `.ok()`, an optional setter, or an omitted protocol field.
- Do not add a mutable capability manager or duplicate PostgreSQL repository path
  authority in core configuration.
- Keep current-main account observation and cache-read isolation unchanged while adding
  Candidate-5 behavior.
- Treat historical migration evidence as superseded provenance, not current authority.
- Update OpenWiki directly for this reset; the user prohibited OpenWiki generation for
  this task.

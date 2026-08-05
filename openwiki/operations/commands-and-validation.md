# Commands And Validation

Use this page to choose the command boundary for current work. `Makefile.toml` owns
implemented task names. This document owns the target behavior for the latest-schema,
runtime-authority, Candidate-5, and local database reset commands.

There are no external or deployed users. Local PostgreSQL data is disposable. Database
acceptance starts from one empty PostgreSQL 18 target and the one unversioned latest
schema. Existing migration commands and harness modes are superseded and must not be used
as acceptance for this reset.

## Task runner authority

The broad repository gate remains:

```sh
cargo make check
```

For a documentation-only or narrow source change, run the smallest relevant check and
state the narrowed scope. The agent automation gate for hosts without full Xcode remains:

```sh
cargo make check-automations
```

`cargo make check-automations` is valid only when the diff does not touch GPUI, its
dependencies, or Apple GPU/build integration. Those surfaces require the broad gate on a
host with full Xcode and Metal tools.

The broad checks do not start a live PostgreSQL fixture. Use the product-native gate for
latest-schema and current-authority acceptance; the migration-era PostgreSQL tasks are
retired.

## Current repository checks

| Purpose | Implemented command |
| --- | --- |
| Broad repository check | `cargo make check` |
| Agent automation check outside GPUI/Apple build surfaces | `cargo make check-automations` |
| Node advisory/provenance/signature audit | `cargo make audit-node` |
| Rust type check | `cargo make check-rust` or `cargo check --all-features --all-targets --workspace` |
| Rust tests | `cargo make test` or `cargo nextest run --workspace --all-targets --all-features` |
| vNext dependency architecture | `cargo make test-vnext-architecture` |
| CLI diagnostic/process matrix | `cargo make test-vnext-cli-diagnostics` |
| Product-native latest-schema and current-authority gate | `cargo make test-vnext-latest-schema` |
| Rust formatting check | `cargo make fmt-rust-check` |
| TOML formatting check | `cargo make fmt-toml-check` |
| Rust lint | `cargo make lint-rust` |
| Read-only Vstyle audit | `cargo make audit-vstyle-rust` |
| Site type check | `cargo make check-node` or `npm --prefix site run check` |
| Site build | `cargo make build` or `npm --prefix site run build` |

The product-native task is the sole canonical latest-schema and current-authority gate.
The other commands are current source checks and do not replace it. Do not cite a
migration-era PostgreSQL subcommand, numbered-ledger result, or historical restore
receipt as latest-schema evidence.

## Latest-schema command boundary

`decodexd bootstrap-latest-schema` owns empty-target bootstrap, and
`decodexd validate-current-authority` owns read-only current-authority validation. The
sole canonical repository gate is `cargo make test-vnext-latest-schema`; it orchestrates
both product commands against one private PostgreSQL 18 target. Do not invent aliases or
split bootstrap and current-authority validation into separate repository gates.

There are three separate operations:

1. **Empty-target bootstrap** resolves the schema-owner credential, proves a clean
   target, executes `crates/decodex-postgres/schema.sql` once in one transaction, verifies
   the result, and commits. If post-schema verification fails, the hidden command emits
   one bounded credential-negative `decodex/bootstrap-authority-report/1` JSON line from
   that transaction. A complete report includes the closed platform checks, all semantic
   authority predicates, and both expected and actual authority digests. If a query stops
   collection, the report retains each completed component in collection order and gives
   the closed failing operation and category. The current and later unavailable components
   are empty or null. The command does not add a second validator or digest-harvest command.
2. **Current-authority validation** is read-only and verifies the live catalog and
   configured runtime authority. It resolves no schema-owner credential and executes no
   DDL.
3. **Local account authority restore** is the hidden
   `decodexd restore-local-account-authority --root ROOT --schema-owner-user USER
   [--schema-owner-credential-env-var ENV]` command. It reads one strict
   `decodex/local-account-authority-restore/1` JSON document from stdin after the daemon
   is stopped and the replacement database has the fresh latest schema. It retains the
   existing same-UID local transport namespace, proves every exact host-vault binding
   before PostgreSQL mutation and again before commit, restores only current account and
   routing rows, and proves exact readback.

The restore stdin is at most 512 KiB and contains at most 512 accounts. Every object
rejects unknown, omitted, and duplicate fields. The command accepts no display label,
token data, SQL, legacy path, or manifest directory. It persists no input document or
receipt. Its only output is one JSON object with `classification` and `account_count`.

Normal `decodexd` serve is not any of these commands. It uses only the runtime credential,
executes zero DDL, and invokes current-authority validation before it retains the store.

No daemon path applies numbered SQL or performs schema administration. The three hidden
commands above are the complete operator surface for latest-schema bootstrap,
current-authority validation, and local account authority restore.

## Source-freeze validation order

After the implementation source is frozen, use this order:

1. reverse scan retired schema names, paths, dependencies, credentials, and commands;
2. bootstrap one fresh PostgreSQL 18 empty target;
3. run the same bootstrap again and prove nonempty refusal with no change;
4. start `decodexd` with only runtime credentials and prove zero DDL/no schema owner;
5. run exact current catalog/configured-authority and adversarial negative checks;
6. parse and execute every changed adapter SQL path against the fresh database;
7. run Candidate-5 domain, transport, and account-observation checks;
8. run the hidden local account authority restore and exact readback scenario; and
9. run the applicable repository aggregate, UI, package, and live exact-build checks.

Do not add Phase A/B, a preparation pass, a digest-only child, a schema receipt, or a
second aggregate. No historical upgrade or migration proof belongs in this order.

## Reverse scan

The reverse scan is read-only and must find no active executable or acceptance reference
to:

- `crates/decodex-postgres/migrations` or numbered `V<integer>__*.sql` files;
- Refinery dependency, macro, runner, target version, or error type;
- `public.refinery_schema_history` or another schema-history relation;
- latest-version constants, `run_through_*`, prefix checks, upgrade branches, or
  migration-source includes/parsers;
- schema-owner credentials in normal daemon configuration/startup;
- migration receipts, finalizers, rollback/fallback code, Phase A/B schema flows, or
  S0/R1/R2 acceptance;
- `SchemaManager`, schema registry, generator pipeline, bootstrap facade, or cutover
  coordinator;
- executable schema ownership in `spikes/vnext-storage`; or
- tests, scripts, task-runner entries, docs, and manifests that require any retired
  mechanism.

The scan may find explicit negative/supersession text in active authority documents and
clearly labeled historical evidence. Classify those hits; do not treat them as executable
drift.

## Empty-target bootstrap validation

Use one isolated PostgreSQL 18 target with data checksums and TCP disabled unless a
separate accepted test requires it. The command must verify the configured Unix-socket
directory, endpoint descriptor identity, expected server UID, and kernel peer UID before
it sends authentication data.

Prove all of these outcomes:

- only the explicit operator invocation resolves the schema-owner credential;
- a clean target matches the accepted fresh-database catalog baseline, apart from
  externally provisioned login roles, with no user-created schema, relation, function,
  type, extension, or other product object;
- `pgcrypto` and the complete latest schema execute in one transaction;
- SQL, verification, disconnect, and injected failure before commit leave no partial
  Decodex schema;
- final enums, relations, constraints, indexes, functions, triggers, dependencies,
  owners, grants, `schema_fingerprint`, and runtime authority are exact;
- every accepted runtime function has the required safe settings and body;
- every accepted trigger is enabled and bound to its final function; and
- the second invocation fails before DDL and leaves the exact catalog unchanged.

Do not run the executable storage spike as bootstrap or proof.

## Runtime-only startup validation

Instrument the database boundary or use an equivalent independent oracle. Start
`decodexd` with the schema-owner credential absent and prove:

- runtime credential resolution only;
- zero DDL and zero extension/schema creation;
- no access to latest schema bytes for execution, numbered SQL, or a schema-history table;
- exact current catalog, dependency, ownership, ACL, function, trigger, semantic, and
  configured-authority checks;
- typed unavailable results for missing, extra, changed, unsafe, unreachable, or
  authentication-failed authority; and
- bounded doctor/status revalidation with no mutation or endpoint repinning.

Adversarial cases cover PUBLIC/runtime grants, inherited and `SET ROLE` paths, object
ownership, DDL, `TRUNCATE`, grant options, trigger bypass, unsafe search path/function
settings, overloads, default ACLs, extension membership, external cascades, rules,
policies, RLS, sequence mutation, trigger/body drift, and fingerprint forgery.

## Adapter SQL validation

Every SQL statement in adapters and tests must target the final schema. Parse and execute
the affected commands against the fresh bootstrapped database. Cover exact request and
response construction, stable rejection, replay, rollback, serialization/deadlock retry,
concurrency, strict readback, and hostile identity cross-links.

Do not keep old/new query branches or fixtures for an old catalog. Remove tests whose only
claim is migration order, upgrade compatibility, or schema-history integrity. Preserve
and retarget tests that protect current domain semantics, ACLs, crash behavior,
reconciliation, or exact readback.

## Candidate-5 validation

The complete boundary is in
[vNext Gates](../specs/vnext-gates.md#candidate-5-quick-task-gate) and
[vNext Authority](../specs/vnext-authority.md#quick-task-thread-establishment).

Validation must prove:

- exact `L0`/`L6` lineage and complete routing evidence;
- one sole Routing Decision account selector;
- atomic first account/profile/session/plan cluster;
- atomic one-winner first Turn and first Message admission;
- selected-account HostCredentialStore pre-spawn fence without re-selection;
- exact Turn locks across ProcessGeneration, thread, and ProviderAttempt effects;
- `Fresh`/`Replayed`/`Rejected`/`Unknown` behavior and definite pre-effect refusal;
- final exact trigger bodies/bindings without a broad starting-session bypass;
- existing same-thread and Context Pack behavior;
- positive-only ProcessGeneration and ProviderAttempt reconciliation;
- same-UID transport ordering, completion, and shutdown; and
- preservation of current-main account observations and cache reads.

For account observation preservation, prove different accounts progress concurrently;
Reset Card work precedes profile observation within one account; repeated wakes coalesce;
publication is revision/cache-generation fenced; and Reset Card/profile queries read
daemon cache or persisted projection without joining, waiting for, or starting provider
refresh work.

## Local database reset validation

Use disposable local state and a test HostCredentialStore. The reviewed operator action
must:

1. stop the daemon and prove the stopped state with the existing local transport
   namespace;
2. supply only credential-negative Account UUID, enabled state, account revision,
   provider binding, credential version/fingerprint, and store binding for every retained
   account, plus routing revision, mode, fixed target, and complete order;
3. bootstrap the latest schema on the replacement database;
4. run `decodexd restore-local-account-authority` with the bounded transient document on
   stdin;
5. prove every `HostCredentialStore::read_exact` binding before PostgreSQL mutation and
   again before commit;
6. prove exact Account Registry and `HostCredentialStore` agreement, every retained
   account's enabled state, revision, and binding, and the routing
   revision/mode/fixed-target/order tuple; and
7. start the daemon only after current-authority verification passes.

For `fixed` mode, require one non-null target that belongs to the retained account set
and complete order. For `balanced` mode, require a null target. Require the order to be a
duplicate-free permutation of all retained accounts, including disabled accounts.
Readback must compare each enabled value and the exact mode, target, order, revision, and
membership as one tuple.

Refuse any target other than the fresh canonical latest schema with zero accounts, the
initial empty routing authority, one active bootstrap execution epoch, empty unrelated
tables, and untouched identity sequences. A refused Keychain fence or readback rolls
back. Do not restore profiles, quotas, operations, conversations, sessions, process
generations, attempts, usage, or historical rows.

Instrument the existing `HostCredentialStore` owner boundary. For credential agreement
only, that owner may perform a confined in-process exact read, recompute and compare the
credential fingerprint and binding, and return only a typed credential-negative
agreement result. The operator action and result must not expose, serialize, copy, log,
persist, rotate, delete, or return token bytes. The check must create no public product
or migration API, generic attestation framework, metadata sidecar, generic import API,
migration state, backup/rollback, receipt/finalizer, or fallback. A failed readback
leaves the daemon stopped.

## Owner path source map

- `crates/decodex-core/`: domain/application contracts, typed paths/configuration, blobs,
  cache, and pure decision values.
- `crates/decodex-protocol/`: exact-current same-UID local protocol and clients.
- `crates/decodex-postgres/schema.sql`: sole executable latest schema.
- `crates/decodex-postgres/src/schema.rs` or the final equivalent module: clean-target
  bootstrap transaction and post-execution verification. The exact file name is an
  implementation choice; its ownership contract is fixed.
- `crates/decodex-postgres/`: current PostgreSQL adapters and read-only authority
  verification.
- `crates/decodex-codex/`: typed app-server adapter and runtime-negotiated capability profiles.
- `crates/decodex-runtime/`: daemon service assembly, Account Service,
  ProcessSupervisor, ProviderAttemptService, account observations, and stateless execution
  coordination.
- `apps/decodexd/`: server composition and exact hidden operator command surface.
- `apps/decodex-cli/` and `apps/decodex-gpui/`: protocol clients.
- `apps/decodex-app/`: credential-negative native account client.
- `apps/decodex/`: frozen v0.2 provenance only.
- `spikes/vnext-storage/`: historical feasibility source only; no executable schema or
  validation owner.
- `site/`, Radar, Publisher, plugins, and automations: auxiliary surfaces with their own
  checks.

## Targeted Rust checks

Common current source checks include:

```sh
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo make test-vnext-architecture
cargo test -p decodex-core --all-targets --all-features
cargo test -p decodex-core -p decodex-protocol -p decodex-postgres -p decodex-codex -p decodex-runtime
```

The task runner exposes one canonical product-native gate for latest-schema bootstrap and
current-authority validation:

```sh
cargo make test-vnext-latest-schema
```

It resolves PostgreSQL 18 through `DECODEX_POSTGRES_18_BINDIR` or `pg_config` on `PATH`,
builds the real `decodexd` binary, and uses only a disposable private target. A test that
still initializes through numbered SQL is not evidence for this reset. On bootstrap
failure, the gate requires the command's one canonical authority report, validates its
closed schema, and retains it with the fixed private gate logs. Query or transport
failure can stop collection only with a closed operation and category. The gate does not
run a second bootstrap to collect diagnostics.

The existing `account-contract` stage first runs the bounded ignored runtime test
`local_account_authority::tests::local_account_restore_command_proves_two_exact_credential_fences_and_readback`,
then runs `postgres_account_routing_contract` against the same private target and
environment. The runtime test uses a module-private read-exact-only credential-store
double; it does not use the live Keychain.

## CLI discovery

The active vNext CLI source starts in `apps/decodex-cli/src/lib.rs`. Current supported
account and Reset Card discovery includes:

```sh
decodex account list
decodex account profile --account-id UUID
decodex account profile --account-id UUID --include-email
decodex fast-mode status
decodex fast-mode set --enabled BOOL
decodex reset-card list --account UUID
decodex reset-card use \
  --account UUID \
  --granted-at UNIX_SECONDS \
  --expires-at UNIX_SECONDS \
  --expected-revision REVISION \
  --idempotency-key KEY \
  --yes
decodex reset-card status --idempotency-key KEY
```

Account profile and Reset Card list are daemon-owned reads. They do not contact the
provider or join account observation work. Create and persist a Reset Card key before
`use`; after a potentially dispatched result, query status with the same key rather than
creating another effect.

No public schema-administration, database migration, or local reset API is implied by
these commands.

## App-server checks

For app-server integration work:

```sh
codex app-server generate-json-schema --experimental --out target/decodex-app-server-schema-check
cargo test -p decodex-codex --all-targets --all-features
cargo test -p decodex-runtime macos_attested_spawn --lib
cargo test -p decodex-runtime live_read_only_probe_negotiates_without_dispatch -- --ignored
```

The runtime supervisor verifies the user's executable identity and generated schema before
spawn, without a fixed Codex release/version allowlist. Raw protocol handles do not leave
ProcessSupervisor. The live probe remains
read-only and does not establish global title discovery or product dispatch.

## Plugin and automation checks

```sh
python3 scripts/config/sync_installable_plugins.py
python3 -m unittest tests/scripts/test_sync_installable_plugins.py
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

Rendering and evaluation do not write scheduler state. Apply automation definitions only
through their owning lifecycle tool and read back every field.

## Static site checks

```sh
npm --prefix site install
npm --prefix site run check
npm --prefix site run build
npm --prefix site run dev
```

## Native macOS app checks

```sh
swift build --package-path apps/decodex-app -c release
swift test --package-path apps/decodex-app -c release
apps/decodex-app/script/build_and_run.sh
scripts/macos/test_decodex_app_stage.sh
python3 -m unittest tests.scripts.test_install_decodex_local_service
```

The staged app contains no daemon, legacy account pool, helper, loopback server,
`:8192` client, schema owner, or database migration tool. The local-service installer
must use the accepted empty-target operator bootstrap for a new database and must start
normal `decodexd` only after bootstrap completes. It does not read an old account pool.

## Radar and Publisher checks

```sh
radar --help
radar validate .agent/automations/radar/cache/site-content/signals
cargo test -p radar
decodex-publisher validate-social
cargo test -p decodex-publisher
```

## Historical command surfaces

Frozen v0.2 commands, old migration harness modes, retained-title freeze commands,
numbered-schema tests, storage-spike execution, and private-artifact validation phases are
historical provenance. Do not run them as latest-schema, Candidate-5, or release
acceptance. A still-useful current domain test must be moved or retargeted to the latest
schema owner.

## Practical checklist

- Schema change: edit only the one latest schema and substantive schema/verification
  owner; run the one product-native gate for fresh bootstrap, second refusal, and both
  runtime-only current-authority validations.
- Adapter SQL change: parse and execute against a fresh latest-schema database; add no
  compatibility branch.
- Candidate-5 change: prove sole selection, atomic admission, exact fences, ambiguity,
  account observation/cache preservation, and transport shutdown.
- Account change: test secret-negative storage/protocol and Registry/store/service
  authority.
- Public projection change: test bounds, redaction, and public/private split.
- Plugin, automation, site, or app change: run that surface's own checks.
- Completion claim: name exact source revision, exact implemented command names, gate
  scope, and any remaining source drift.

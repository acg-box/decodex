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

The current task runner still contains migration-era PostgreSQL checks. Until source and
task-runner alignment lands, a passing broad command does not prove the new latest-schema
contract. The implementation change must retire old tasks or change their behavior under
the same reviewed source change.

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
| Rust formatting check | `cargo make fmt-rust-check` |
| TOML formatting check | `cargo make fmt-toml-check` |
| Rust lint | `cargo make lint-rust` |
| Read-only Vstyle audit | `cargo make audit-vstyle-rust` |
| Site type check | `cargo make check-node` or `npm --prefix site run check` |
| Site build | `cargo make build` or `npm --prefix site run build` |

These commands are current source checks. They do not replace the new database gates
below. Do not cite a migration-era PostgreSQL subcommand, numbered-ledger result, or
historical restore receipt as latest-schema evidence.

## Latest-schema command boundary

The exact command names for empty-target bootstrap, current-authority validation, and the
operator-only local reset are implementation-owned. They are not yet authoritative.
Implementers must add or rename commands and task-runner entries together with their
source and contract tests. Do not invent aliases in documentation or preserve old names
for compatibility.

There are three separate operations:

1. **Empty-target bootstrap** resolves the schema-owner credential, proves a clean
   target, executes `crates/decodex-postgres/schema.sql` once in one transaction, verifies
   the result, and commits.
2. **Current-authority validation** is read-only and verifies the live catalog and
   configured runtime authority. It resolves no schema-owner credential and executes no
   DDL.
3. **Local database reset** is one reviewed operator-only action that stops the daemon,
   preserves the complete credential-negative account and routing tuple, including each
   enabled state and the mode-valid fixed target, replaces or directly transforms the
   local database, bootstraps when needed, rebinds the exact tuple to unchanged
   host-vault records, proves exact readback, and starts the daemon.

Normal `decodexd` serve is not any of these commands. It uses only the runtime credential,
executes zero DDL, and invokes current-authority validation before it retains the store.

The current hidden `provision-local` behavior and any daemon path that applies numbered
SQL are superseded implementation. They do not define the future command spelling or
contract.

## Source-freeze validation order

After the implementation source is frozen, use this order:

1. reverse scan retired schema names, paths, dependencies, credentials, and commands;
2. bootstrap one fresh PostgreSQL 18 empty target;
3. run the same bootstrap again and prove nonempty refusal with no change;
4. start `decodexd` with only runtime credentials and prove zero DDL/no schema owner;
5. run exact current catalog/configured-authority and adversarial negative checks;
6. parse and execute every changed adapter SQL path against the fresh database;
7. run Candidate-5 domain, transport, and account-observation checks;
8. run the direct local reset/rebind/readback scenario; and
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

1. stop the daemon;
2. capture only credential-negative Account UUID, enabled state, account revision,
   provider binding, credential version/fingerprint, and store binding for every retained
   account, plus routing revision, mode, fixed target, and complete order;
3. replace or directly transform the database;
4. bootstrap the latest schema when the target was recreated;
5. restore or rebind the exact captured tuple against unchanged host-vault records;
6. prove exact Account Registry and `HostCredentialStore` agreement, every retained
   account's enabled state, revision, and binding, and the routing
   revision/mode/fixed-target/order tuple; and
7. start the daemon only after current-authority verification passes.

For `fixed` mode, require one non-null target that belongs to the retained account set
and complete order. For `balanced` mode, require a null target. Require the order to be a
duplicate-free permutation of all retained accounts, including disabled accounts.
Readback must compare each enabled value and the exact mode, target, order, revision, and
membership as one tuple.

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
- `crates/decodex-codex/`: typed app-server adapter and exact-build capability profiles.
- `crates/decodex-runtime/`: daemon service assembly, Account Service,
  ProcessSupervisor, ProviderAttemptService, account observations, and stateless execution
  coordination.
- `apps/decodexd/`: server composition and implementation-owned operator command surface.
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

After implementation, the task runner must expose one canonical latest-schema/bootstrap
gate and one canonical current-authority/runtime gate. Their names remain
implementation-owned. A test that still initializes through numbered SQL is not evidence
for this reset.

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

The exact-build supervisor verifies executable identity and generated schema before
spawn. Raw protocol handles do not leave ProcessSupervisor. The live probe remains
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
  owner; run fresh bootstrap, second refusal, runtime-only startup, and current-authority
  gates.
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

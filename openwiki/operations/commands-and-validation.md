# Commands And Validation

Use this page when deciding which command validates a change. It summarizes current task-runner authority and maps test families to source areas.

The XY-1403 private-artifact retirement takes effect only at the exact
[repository effective point](../specs/private-artifact/decision.md#repository-effective-point).
At and after that point, the private-artifact archive defines no command or
validation authority.

## Task runner authority

`Makefile.toml` owns repository task names. The broad readiness gate is:

```sh
cargo make check
```

It depends on the Node supply-chain audit, build, Node checks, Rust checks,
formatting, lint, and tests (`Makefile.toml`). Use it before claiming broad readiness.
For documentation-only or narrow source changes, run the smallest relevant checks
and state the narrowed scope.

The agent automation portfolio has a headless gate for hosts without full Xcode:

```sh
cargo make check-automations
```

It excludes `decodex-gpui` from Rust check, lint, and test work but preserves the
other repository gates. It installs the exact lock with lifecycle scripts disabled,
checks npm advisories at high severity, verifies registry signatures for the resolved
site graph, and audits every lockfile source and integrity value. The audit requires
npm 11.17.0, Node 22.12.0 or newer, registry.npmjs.org package sources, SHA-512
integrity, the exact reviewed lifecycle-script allowlist, and the pinned native and
platform package identity set. It is valid only when the
pull-request diff does not touch `decodex-gpui`, its dependencies, or Apple GPU/build
integration. Changes on those surfaces require `cargo make check` on a host with full
Xcode and Metal tools. The upstream validator discovers a usable full Xcode and
scopes `DEVELOPER_DIR` to that subprocess. Both aggregates include the Node audit.
Run either aggregate on a clean committed tree because PostgreSQL authority tests
bind their evidence to the exact commit and tree.

## Main gates

| Purpose | Command |
| --- | --- |
| Broad repo check | `cargo make check` |
| Agent automation check outside GPUI/Apple build surfaces | `cargo make check-automations` |
| Site advisory, provenance, and registry-signature audit | `cargo make audit-node` |
| Rust type check | `cargo make check-rust` or `cargo check --all-features --all-targets --workspace` |
| Rust tests | `cargo make test` or `cargo nextest run --workspace --all-targets --all-features` |
| XY-1306 path/config/blob/cache foundation | `cargo test -p decodex-core --all-targets --all-features` |
| XY-1307 daemon bootstrap and doctor protocol | `cargo test -p decodex-core -p decodex-protocol -p decodex-postgres -p decodex-runtime -p decodexd --all-targets --all-features` |
| XY-1308 CLI diagnostic and local Git command matrix | `cargo make test-vnext-cli-diagnostics` |
| vNext dependency architecture | `cargo make test-vnext-architecture` |
| vNext PostgreSQL store, Conversation history, blobs, and Context Packs | `cargo make test-vnext-postgres-store` |
| XY-1345 isolated exact-command authority proof | `python3 scripts/vnext/exact_command_prototype.py` |
| vNext storage feasibility proof | `cargo make test-vnext-storage-proof` |
| Rust formatting | `cargo make fmt-rust-check` |
| Canonical gate contract | `cargo make test-gate-contract` |
| TOML formatting | `cargo make fmt-toml-check` |
| Rust lint | `cargo make lint-rust` |
| Read-only Vstyle audit | `cargo make audit-vstyle-rust` |
| Site type check | `cargo make check-node` or `npm --prefix site run check` |
| Site build | `cargo make build` or `npm --prefix site run build` |

`cargo make test` runs `cargo nextest run --workspace --all-targets --all-features`, the
canonical gate contract tests, the vNext architecture test, and the XY-1308 CLI
process matrix (`Makefile.toml`). The active `apps/decodex-cli` uses the server only
for `status`, `doctor`, and `reset-card`. Its manual-authority `commit` and exact-base/head `land`
commands are local Git authority and do not use Decodex server, planner, runtime,
MCP, Linear, or tracker state. Rust compilation remains pinned by
`rust-toolchain.toml` to `1.97.0`. The formatting tasks separately invoke
`rustup run nightly-2026-07-16 cargo fmt`, which preserves the nightly-only options in
`.rustfmt.toml` without depending on the mutable `nightly` alias. Supported hosts install
that exact formatter toolchain once; formatting fails closed when it is unavailable:

```sh
rustup toolchain install nightly-2026-07-16 --profile minimal --component cargo --component rustfmt
```

## Retired private-artifact delivery contracts

The archived
[operations and delivery module](../specs/private-artifact/operations-delivery.md)
preserves the former contracts. Every item in this table is historical and
non-executable:

| Former contract | Disposition |
| --- | --- |
| A0/A1/B/D0a/C/D and their delivery edges | Retired. They are not phases, dependencies, issue work, or future obligations. |
| CORE-FREEZE | Retired. It is not a receipt boundary or current or future task. |
| ACC | Retired. It cannot authorize acceptance-source changes. |
| `check-vnext-postgres-preparation` | Retired package proposal. Do not add it from the archive. |
| Package-defined `test-vnext-retained-title-core` behavior | Retired. The existing task name and current behavior come only from checked-in task, wrapper, and runbook authority. |
| Single mechanical preparation pass | Retired. Do not run or recreate it from the archive. |
| Unified complete validation protocol | Retired. It is not an acceptance or enablement gate. |

The immutable [XY-1368 retained-title freeze](../specs/xy-1368-retained-title-freeze.md) is V22
historical evidence only. It is not current command or task-runner authority.
Current executable command names and behavior come from `Makefile.toml`, the current
wrapper source, and the unchanged
[XY-1368 validation runbook](xy-1368-retained-title-validation.md). Do not infer
retired package behavior from those current surfaces.

## Same-UID Unix transport validation

XY-1399 A-prime was the source-only design ancestor. The current integrated implementation
ports that transport onto exact-current protocol V2.0 and the shared Reset Card service. Validation
must bind all results to the exact candidate tree and cover macOS and Linux. It validates:

- fixed staging-name and canonical-name stale recovery under the persistent single-link
  namespace lock;
- stage bind, exact mode, captured device/inode/owner/mode/link-count identity, exactly
  one socket link, and same-directory descriptor-relative `renameat` publication;
- directory, lock, and socket replacement at publication, server admission, client
  reconnect, and cleanup;
- client and server kernel peer credentials and exact effective-UID equality;
- WebSocket `/v1/ws` with exact-current V2.0, without TCP, Axum, self-connect, watchdog, or
  compatibility fallback;
- concurrent legitimate daemons, active sessions, in-flight commands, child panic,
  absolute-deadline cancellation, deterministic termination receipt readback, explicit
  `join_next_with_id` harvesting through empty, and zero owned work before cleanup;
- immediate Reset Card work-admission closure at shutdown and settlement of every already
  registered blocking provider operation before namespace cleanup;
- SIGINT and SIGTERM graceful cleanup and SIGKILL stale-socket recovery;
- exact cleanup refusal, listener close, and namespace-lock release order; and
- reverse dependency isolation for remote/cross-UID transport, PKI, PostgreSQL end-user
  authentication, routing, UI, packaging, release, and unrelated production dispatch.

The integrated caller conversion removes `LoopbackEndpoint`, local-profile `address`,
URL-based retained-session construction, the transport-level `InvalidEndpoint`, active
local-daemon TCP socket fixtures, `BoundServer::address`, TCP V1 URIs, the fixed local
`127.0.0.1:49152` endpoint, dependency-only Axum use, and the Axum workspace edge. Inert
remote-profile port data and the GPUI `CompatibilityReason::InvalidEndpoint` display
classification remain outside the local transport implementation.
`crates/decodex-protocol/tests/local_transport_authority.rs`
owns namespace, stale recovery, replacement, permission, peer, and path-length coverage.
`crates/decodex-runtime/tests/websocket_protocol.rs` owns WebSocket continuity and
zero-survivor service settlement. `apps/decodexd/tests/signal_shutdown.rs` owns real
process signal and crash-recovery behavior.

## XY-1402 source-only validation boundary

XY-1402 is pre-core-freeze source work. Do not run a formatter, build, compiler,
lint or static analysis, migration or SQL parser, test, fixture, generator,
service, VM, UI or Accessibility check, live Codex experiment, account operation,
or provider effect for its candidate. Bounded source inspection and the required
signed commit receipt are not executable acceptance.

V25 adds the execution route and wait enum vocabulary in a separate committed
transaction. V26 is the current execution-coordination cutover. It removes the drained V12
ManagedRun-local submitted-turn and effect-barrier authority. Its source inventory
contains 80 relations, 182 functions, 74 safety functions, 146
safety/state/retention triggers, and 70 runtime-callable functions. The accepted
schema and configured-authority digests stay frozen at the V22 boundary until the
later unified gate derives and verifies the integrated V26 values.

The later gate must bind its results to the exact candidate tree. It must run the
complete [XY-1402 deferred acceptance matrix](../specs/execution-coordinator-authority.md#deferred-acceptance-matrix).
That matrix includes clean and populated migration, drain, historical
cross-link, and ambiguity falsifiers, S0/R1/R2 manifests, ACL closure, both
consumer shapes, route causes, quota separation and aging, RuntimeSession
continuity, ProcessGeneration fencing, ProviderAttempt capability consumption
and ambiguity, same-UID transport, concurrency, hostile cross-links, and reverse
production isolation.

## Vstyle audit authority

Vstyle is an explicit read-only audit and is not part of the blocking `lint` or `check`
aggregates. The ordinary `lint-fix` aggregate contains only Clippy fixing; the repository
provides no Vstyle mutation task. Run the governed audit directly:

```sh
cargo make audit-vstyle-rust
```

`config/vstyle-rust-audit.json` pins `vibe-style` 0.2.3 to source commit
`3a0959eac5363c4c427382bae1d80d87ecadb702`, attests the complete implemented Rust rule
inventory, and records the reviewed current baseline of 184 findings, including seven
manual findings. Supported hosts install that exact revision:

```sh
cargo install --git https://github.com/hack-ink/vibe-style.git \
  --rev 3a0959eac5363c4c427382bae1d80d87ecadb702 \
  --locked --force vibe-style
```

The audit fails closed when the executable version, source revision, build target, rule
inventory, output grammar, or governance deadline differs from the contract. It also fails
when normalized finding counts increase. Resolved baseline findings are reported without
blocking so maintainers can refresh the baseline through review.

Vstyle 0.2.3 does not expose structured curate output. The smallest repository-owned
boundary therefore accepts only its exact finding and summary line grammar and rejects
every unexpected nonempty line. Finding identity is normalized by path, rule, message,
fixability, and multiplicity; line and column numbers are deliberately excluded so an
unrelated line shift does not appear as a regression. The baseline owner is the Decodex
repository maintainers. It must be reevaluated by 2026-08-15, whenever executable or rule
identity changes, whenever the accepted baseline changes, and before any scope is promoted
to blocking.

The current CLI matrix builds the real `decodex` binary and uses the fixed owner-only Unix
namespace. It proves status/doctor, stable identity mismatch,
disconnection, malformed/missing profile configuration, unsafe server-host paths,
database unavailability, plugin/vault/blob unknown states, and redaction. Protocol unit
fixtures separately force wrong major/minor, malformed/oversized response, timeout, and
untrusted server-text cases.

Run the focused reset-card PostgreSQL crash/reclaim contract in its own disposable
PostgreSQL 18 cluster:

```sh
python3 scripts/vnext/postgres_store_test.py --focus-reset-cards
```

This contract proves generic-claim isolation, private exact-ID binding, effect fencing,
lease expiry and reclaim, same-key receipt survival, reconciliation, and terminal status.

PostgreSQL authority digest changes use an explicit derivation phase followed by one acceptance
phase:

```sh
# Run the one-shot R1 prerequisite gate only when the Manager authorizes this exact clean tree.
python3 scripts/vnext/postgres_store_test.py \
  --capture-authority-restore-prerequisite-v2 \
  /absolute/private/directory/postgres-restore-prerequisite-r1.json

# Run Phase A from one clean committed repaired pre-digest tree.
python3 scripts/vnext/postgres_store_test.py \
  --capture-authority-candidate /absolute/private/directory/postgres-authority-candidate.json

# Follow the receipt's zero-, one-, or two-array transition, then run Phase B.
python3 scripts/vnext/postgres_store_test.py \
  --accept-authority-candidate \
  /absolute/private/directory/postgres-authority-candidate.json \
  /absolute/private/directory/postgres-authority-acceptance.json
```

`--capture-authority-restore-prerequisite-v2` is the only spelling for the replacement gate. The v1
spelling has no alias. This gate is not a focused mode, Phase A, Phase B, or the aggregate. Its
output path must be absolute, outside the source tree, in an exact operator-owned private directory,
and absent before the command starts. The Manager authorizes one invocation on one exact clean HEAD
and tree. The gate does not provide cross-process once-only state.

One state owner records this ordered execution prefix: `cli`, `output_contract`,
`source_binding_preflight`, `temporary_root`, `tool_discovery`, `toolchain_preflight`,
`private_work`, `cluster_init`, `cluster_start`, `role_setup`, `source_binding_gate_start`,
`toolchain_gate_start`, `server_version`, `definition_binding`, `source_database_created`,
`source_migrated`, `source_provisioned`, `source_populated`, `source_semantic_authority`,
`source_archive_created`, `archive_declaration_guarded`, `restore_database_fresh_template0`,
`restore_pgcrypto_absent`, `restore_prerequisite_created`, `restored_once`,
`restored_once_semantic_authority`, `semantic_authority_equal`, `invocation_policy`,
`source_binding_gate_end`, `toolchain_gate_end`, `privacy_validation`, and
`stopped_after_restored_once`. A receipt cannot claim a checkpoint until its action succeeds.

The separate lifecycle owners are `cluster_stop`, `private_work_cleanup`, `cleanup_finalization`,
`receipt_validation`, `receipt_source_binding`, and `receipt_publication`. Actual lifecycle state
derives the ordered required cleanup sequence. `cluster_stop` is present only after cluster stop
becomes applicable. `private_work_cleanup` is present only after private work exists. The only valid
sequences are empty, `private_work_cleanup`, or `cluster_stop` then `private_work_cleanup`. Each
required owner has pending, active, or completed state. `cleanup_finalization` is always an explicit
fail-closed transition after that sequence.

The exact reason set is `contract_invalid`, `authority_unavailable`, `changed`, `operation_failed`,
`archive_declaration_invalid`, `target_not_fresh`, `duplicate_invocation`,
`invocation_policy_failed`, `semantic_authority_changed`, `cleanup_failed`, `receipt_invalid`,
`publication_failed`, `interrupted`, and `harness_corruption`. The definition binds the exact
allowed checkpoint and reason pairs. The expected pairs are:

- `cli`, `output_contract`, `temporary_root`, `definition_binding`, `privacy_validation`, and
  `stopped_after_restored_once` use `contract_invalid`.
- `source_binding_preflight`, `tool_discovery`, `toolchain_preflight`, and `server_version` use
  `authority_unavailable`.
- `source_binding_gate_start`, `source_binding_gate_end`, `toolchain_gate_start`, and
  `toolchain_gate_end` use `authority_unavailable` or `changed`.
- `private_work`, `cluster_init`, `cluster_start`, `role_setup`,
  `restore_database_fresh_template0`, `restore_prerequisite_created`, and `restored_once` use
  `operation_failed`.
- `source_database_created`, `source_migrated`, `source_provisioned`, `source_populated`,
  `source_semantic_authority`, `source_archive_created`, and `restored_once_semantic_authority` use
  `operation_failed` or `duplicate_invocation`.
- `archive_declaration_guarded` uses `archive_declaration_invalid` or `duplicate_invocation`.
- `restore_pgcrypto_absent` uses `operation_failed` or `target_not_fresh`.
- `semantic_authority_equal` uses `semantic_authority_changed`.
- `invocation_policy` uses `duplicate_invocation` or `invocation_policy_failed`.
- `cluster_stop` and `private_work_cleanup` use `cleanup_failed`.
- `cleanup_finalization` has no expected operation failure.
- `receipt_validation` uses `receipt_invalid`; `receipt_source_binding` uses
  `authority_unavailable` or `changed`; and `receipt_publication` uses `publication_failed`.

Every owner also permits `interrupted` and `harness_corruption`. An unexpected assertion, type,
key, or invariant failure is `harness_corruption` at the active owner. The first primary failure is
immutable. Before the first action, after an action before transition, between actions, and during
finalization, one active or pending cleanup owner remains authoritative. Cleanup has fixed status and
can become primary only when no execution primary exists. An execution primary keeps only the fixed
secondary `cleanup_failed` reason. `cleanup_status=passed` proves that the completed cleanup sequence
equals the required sequence and that finalization completed. Publication cannot relabel an earlier
primary.

The gate creates S0 once with the unchanged authority-mode setup. It migrates, provisions,
populates, and runs the complete Rust semantic-authority owner once. It creates one custom dump.
Before it creates R1, it uses the selected PostgreSQL 18 `pg_restore` to inspect the private TOC.
The closed PostgreSQL 18 list parser requires one active `pgcrypto` `EXTENSION` declaration. It
rejects an absent, duplicate, disabled, malformed, or ambiguous declaration. The guard does not
retain or publish a TOC line, owner, ACL, OID, role, SQL, connection, path, or discovered object.

After the guard passes, the shared restore helper creates R1 from `template0`. It connects as the
canonical migration role and proves that `pgcrypto` is absent. It then executes exactly
`CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public VERSION '1.4';`. The ordinary
`pg_restore --exit-on-error` command stays under the bootstrap identity. It does not use `--role`,
`--use-set-session-authorization`, `--no-owner`, or `--no-acl`. The helper does not run a migration
or runtime provisioning after restore. The full Rust semantic-authority owner runs once at R1.
The same guard and helper own each future Phase A R1 and R2 target.

The gate stops after R1 for every outcome. It does not create R2, derive a manifest digest, publish
an authority candidate, run Phase B, or run the aggregate. A pass publishes one immutable,
create-only, mode-0600, file-fsynced and directory-fsynced
`decodex/postgres-restore-prerequisite-r1-gate/2` receipt. The receipt has `acceptance=false`. It
contains the exact source binding, PostgreSQL toolchain fingerprint, definition fingerprint
`53bb20b8e43a6199c3aa578269cee8b941ed549fd8f10db0dce361a03016524a`, the complete validated
checkpoint prefix, the exact required and completed cleanup-owner sequences, completed cleanup
finalization, and fixed invocation-policy Booleans. The definition schema is
`decodex/postgres-restore-prerequisite-r1-definition/2`.

A failure uses `decodex/postgres-restore-prerequisite-r1-diagnostic/2`. It contains the validated
source binding, or null before source validation, the immutable primary checkpoint and reason, the
validated completed prefix, fixed cleanup status, and the optional fixed secondary cleanup reason.
It also carries the exact required and completed cleanup-owner sequences, completed finalization,
and the fixed `failure_document_repaired` Boolean. It includes the existing closed
semantic-authority diagnostic only when a semantic owner has the primary failure. It cannot contain
raw exception text, a command, child output, environment, path, selected tool name, database, role,
owner, ACL, OID, SQL, connection, TOC content, catalog row, count, or discovered identity. Failure
document construction and repair belong to `receipt_validation`. Incomplete or corrupt cleanup
state produces one fixed privacy-safe repaired diagnostic and preserves a valid earlier primary.
When the output contract is valid, the owner tries to publish one create-only failure receipt after
cleanup. It also writes the same canonical diagnostic to standard error for publication-failure
recovery. A fixed `receipt_validation/harness_corruption` fallback remains available if normal
construction or durable publication fails. The raw-error `StageOrchestrator` remains separate.

The v1 gate ran once and returned ownerless `gate/stage_failed` evidence. That result did not prove
that candidate 3 reached the archive guard, prerequisite, restore, or R1 semantic owner. Candidate 3
remains frozen and unadjudicated. The three-rejected-candidate threshold is not crossed unless one
source-bound replacement result exercises and rejects candidate 3. A v2 pass authorizes only a
later decision about revised Phase A. This v2 source is unexecuted, and it makes no acceptance
claim.

Phase A must start from one clean committed repaired tree. Its mismatch array is the exact ordered
subset of `schema`, then `configured_authority`: it may be empty, contain either one component, or
contain both in that canonical order. No unrelated component, duplicate, or reordered mismatch is
valid. For one or two mismatches, the operator creates one clean direct single-parent child that
changes exactly the reported digest array or arrays in `authority.rs`; every unreported digest and
every other path remains byte-identical. For zero mismatches, Phase B uses the same clean HEAD and
tree without an intervening commit. No particular commit hash is prescribed: the receipt and Phase
B consumer bind and verify the actual Phase A commit/tree, candidate-receipt hash, clean Phase B
commit/tree, exact mismatch set/order, and the corresponding same-tree or digest-only-child shape.

The capture-only mode migrates and provisions an isolated PostgreSQL 18 database, captures raw
source S0, first-restore R1, and second-restore R2 evidence, and separately proves the exact
V13-to-V22 one-grantee runtime ACL delta from raw catalogs. The same immutable
semantic-authority contract used by production readiness must pass at S0, R1, and R2 before the
command atomically publishes
`decodex/postgres-authority-candidate/3`; the Phase A mismatch set must be exactly the canonical
zero-, one-, or two-component subset described above, while complete, unique, resolved manifests,
migration ledger,
semantic state, population, runtime-authority shape, and semantic evidence satisfy both restore
edges. Raw manifests
and temporary cluster state are not retained in the receipt. Capture, PostgreSQL shutdown/removal,
and final source binding complete before publication. A complete fsynced mode-0600 temporary receipt
is published by one create-only hard link; that link is the commit point and never overwrites or
rolls back the final path. Directory fsync completes the normal-success durability claim, but any
failure or interruption after the link is an ambiguous producer outcome resolved by reading the
immutable receipt. Exit status and stdout are not evidence. The receipt has `acceptance=false`; it
never substitutes for the normal aggregate. Phase B alone validates the exact immutable receipt,
its hash and Phase A HEAD/tree, and the exact transition authorized by the mismatch array. It then
repeats S0→R1→R2. Each checkpoint must contain the Rust-owned versioned definition, its ordered
Boolean observations, and its emitted fingerprint. Python independently recomputes the
domain-separated, length-prefixed SHA-256 fingerprint and compares it with the emitted value and
the one supported value. It rejects malformed, missing, duplicate, extra, reordered, non-Boolean,
false, or checkpoint-divergent evidence without a copied predicate inventory or Rust source
inspection. Phase B requires every semantic predicate and restore edge to pass with zero digest
mismatches, and publishes
the only `acceptance=true` receipt bound to both trees and the Phase A receipt hash. Existing
malformed, substituted, duplicate, or mismatched receipts fail closed. A zero-mismatch acceptance
receipt is freshly emitted from the unchanged clean Phase A HEAD/tree and is explicitly bound to
the Phase A receipt; existing receipts remain provenance only and cannot attest a source outside
their recorded binding. For one or two mismatches, any unreported array change or other source
delta invalidates the candidate.

Phase A candidate capture uses a dedicated semantic parser path. It first validates the complete
artifact shape, component structure, database binding, Rust-emitted definition, ordered Boolean
observations, emitted fingerprint, independently recomputed fingerprint, and supported fingerprint.
If one or more observations are false, it raises the exception-only
`decodex/postgres-semantic-authority-diagnostic/2` diagnostic. The diagnostic has only `schema`,
`source_binding`, `checkpoint`, `definition_fingerprint`, and `failures`. The source binding has
only the exact lowercase 40-hex `head` and `tree`. The checkpoint is `source`, `restored_once`, or
`restored_twice`. The nonempty failure array is complete and uses definition order. Each item has
only the fingerprint-bound `predicate` and its fixed Rust-defined `failure_policy`. Canonical JSON
uses sorted keys, compact separators, and ASCII escaping. The diagnostic does not contain passed
observations, a concrete runtime-derived failure class, SQL, catalog or role data, counts, paths,
raw evidence or errors, candidate mismatches, or schema, configured-authority, manifest, or
mismatch digest values. The supported `definition_fingerprint` is its only digest. This branch
stops before semantic summary hashing, digest derivation, mismatch construction, and receipt
publication. Malformed evidence remains `artifact_malformed` and does not echo attempted
predicate text. The shared retained-title loader keeps its immediate all-pass requirement.

The current [XY-1368 retained-title validation](xy-1368-retained-title-validation.md) documents the
two historical V22-era partial-boundary command surfaces that still exist in source. Neither
command authorizes full-check publication or production enablement, and neither implements the
retired private-artifact delivery contracts above.

The normal aggregate uses one explicit stage report. Configuration and cluster preflight are fatal:
mode/argument validation, clean source binding, temporary-root validation, PostgreSQL tool
discovery, temporary cluster initialization/start, and base-role creation must all pass before
semantic work is scheduled. Phase A/B instead validate their private output and receipt/source
lineage directly, outside the aggregate scheduler. Meaningful aggregate suites then report `passed`,
`failed`, or `blocked`. Ordinary `TestFailure` leaves independent branches schedulable but blocks
every declared consumer of the failed prerequisite. RoleProfile, RuntimeSession, ManagedRun V26,
migration-boundary,
blob-restart, primary-store, managed-repository, account-composition, bootstrap/doctor, collation,
authority safety, hostile-search-path, primary restore, redaction, default-ACL restore,
authority-drift, and final-evidence work are all represented. A restore-owning suite cannot pass
when one of its required nested captures, restores, parity checks, or production checks failed or
became unavailable, so its consumers are blocked truthfully. One private live-doctor mutation SQL
executor owns the ordinary, role-as, and secret-bearing mutation subprocesses, their dispatch and
completion facts, output handling, and cleanup; the coordinator owns doctor readiness and the
doctor child only. Every mutation and doctor child receives bounded terminate, kill-fallback, and
reap attempts on every exit; an indeterminate or unreaped child is harness corruption. The probe
and fixture restoration are separate stages. A failed ordinary `Popen` is pre-dispatch and blocks
restoration; successful `Popen` owning SQL in argv makes delivery possible, so every later failure
remains restoration-eligible. A secret mutation completes its fail-closed logging prelude before
dispatch, becomes may-have-dispatched immediately before the first mutation-frame payload write,
and remains eligible after any later write, flush, timeout, protocol, exit, or cleanup failure.
Successful exit means only command acknowledged, not exact server receipt or non-vacuous mutation
application; an optional postcondition query is separate evidence. The scheduler consumes exactly
one restoration claim per shared-fixture attempt: pre-dispatch probe failure blocks restoration,
eligible probe failure still attempts it, and the next shared-fixture probe depends on successful
restoration. Assertion/type/key failures, invalid stage/report state, source-binding failures,
redaction failures, and other unexpected exceptions are harness corruption and stop new scheduling.
A private work directory is
created with mode 0700 under `/private/tmp` by default on macOS; the existing validated
`DECODEX_TEST_TEMP_ROOT` override remains available. Before cluster initialization, the harness
rejects any resolved workspace whose final `.s.PGSQL.<port>` pathname plus terminating NUL exceeds
the portable 104-byte Unix-socket bound. The directory is cleaned directly when preflight fails
before cluster start, with cleanup failure remaining subordinate. If `pg_ctl start` fails, its
primary `TestFailure` includes a bounded, secret-marker-redacted tail of `postgres.log` before the
stopped cluster is removed; no log or cluster retention is introduced. After PostgreSQL has
started, teardown and final stage-report emission still run. The semantic/stage failure is selected
before aggregate output and report emission, so cleanup or emission failures are recorded without
replacing it. The normal aggregate emits `decodex/postgres-aggregate-stage-report/1`. The focused
ManagedRun mode emits `decodex/postgres-managed-run-v26-stage-report/1`. Other focused modes and
Phase A/B capture modes preserve their direct output or receipt behavior.

An unavailable or incomplete raw schema/authority component publishes no receipt. Before its private
cluster is removed, the capture emits a versioned, bounded diagnostic to the operator-owned combined
log containing the exact source/database binding, component status, manifest hash and row count when
present, and at most eight unresolved dependency kind/identity/reference records. In this mode only,
database failures contain SQLSTATE and primary message; non-database failures use fixed generic text.
Malformed artifacts report only classification, expected binding, byte length/hash when readable,
and a bounded parser error. Diagnostic text and identities are length-bounded and secret-marker
redacted; full manifests and raw contracts are never emitted.
Semantic `/2` diagnostics contain only the schema, exact source binding, canonical checkpoint,
supported definition fingerprint, and complete definition-ordered failure objects. Each failure
contains only its false canonical predicate name and fixed failure policy. These diagnostics never
contain SQL, ACL bodies, object identities, connection data, paths, or other digest values.

The PostgreSQL integration command uses an isolated PostgreSQL 18 cluster and fixture-only roles.
Its authority matrix contains 28 unsafe roots and six incompatible roots. The new unsafe root adds
a migration-owned, runtime-executable `public` `SECURITY DEFINER` routine. Direct runtime
ManagedRun mutation is rejected first. The routine then performs one valid ManagedRun
revision/divergence mutation, while the Decodex relation, routine, trigger, rule, and policy
inventories stay unchanged. Production verification must reject this unexpected runtime entry.

Runtime entry closure evaluates the login identity and every inherited or `SET ROLE`-reachable
identity. Effective privileges include `PUBLIC` and column grants. The verifier rejects unexpected
privileges on non-system relation-like objects and unexpected runtime-executable non-system
security-definer normal functions, procedures, or window functions. Aggregate rows are excluded
because PostgreSQL `CREATE AGGREGATE` has no `SECURITY DEFINER` capability. The one external
execution dependency is exact `public.digest(bytea,text)` from `pgcrypto` 1.4, including extension
membership, namespace, owner relationship, metadata, and ACL. Existing scenarios continue to cover
direct and indirect authority, DDL, relation/ledger/sequence mutation, grant options, trigger drift,
extension control, canonical-function drift, external cascades, ledger tampering, and absent
`pgcrypto`.

The following historical V9 detail remains part of the retained test surface.
The XY-1267/XY-1307 integration command and XY-1264 storage proof are intentionally separate because they require an intended macOS host with one PostgreSQL 18 distribution. Each creates and removes its own isolated temporary checksummed cluster with TCP disabled and never enumerates or changes an existing service. The PostgreSQL command provisions fixture-only migration/runtime roles, proves least-privilege daemon bootstrap, and rejects 27 unsafe roots covering direct, inherited, NOINHERIT/SET-only, and membership-admin paths to forbidden role attributes, PostgreSQL 18 namespace-object ownership (including distinct collation, conversion, operator, and text-search cases), DDL, table/ledger/sequence mutation, grant options, `session_replication_role` SET/ALTER SYSTEM, retention bypass, trigger drift, extension-member control, an indirect public-function trigger, and a genuinely additional function. It closes every runtime-callable Decodex function over exact signatures, overloads, metadata, settings, and canonical source. At the frozen V9 boundary, all 72 shipped functions have the exact secure `pg_catalog, decodex` function-local search path, and all 52 non-internal shipped trigger bindings—including deferred constraint triggers—have exact catalog attestations. The 16 shipped security definers comprise three V3 cursor/history functions, eleven V5-V7 Project/Policy/Program/Objective command entrypoints, and two V9 RoleProfile command entrypoints; runtime cannot insert cursors or execute capture directly. The additional fixture-only seventy-third function is migration-owned, runtime-executable, `SECURITY DEFINER`, configured with an unsafe setting, and is invoked as runtime to perform owner-authority trigger DDL before fixture restoration and independent rejection. The separate substitution fixture replaces a shipped safety body without changing its signature. The indirect-trigger fixture proves runtime DML executes a public definer function despite direct `EXECUTE`, protected-table `UPDATE`, and `TRIGGER` all being denied. The extension fixture proves a public runtime-owned extension can transactionally drop it. Six incompatible roots cover missing ledger SELECT, canonical-function drift, a dropped credential constraint with demonstrated credential insertion, an external child cascade with demonstrated runtime-mediated deletion, a same-count tampered migration ledger, and absent `pgcrypto`. The canonical PostgreSQL 18 schema manifest closes defaults, constraints on both foreign-key sides, indexes, enums, and internal constraint-trigger semantics. Descriptor-pinned socket unit fixtures reject a same-UID pre-planted endpoint in a world-writable configured directory, a mismatched operator UID pin, replaced ancestors, replaced endpoints, and deterministic replacement between precheck and failed connect; an unchanged secure stale socket maps to unreachable. An isolated daemon fixture starts Ready, replaces the configured endpoint, and proves a fresh V1.3 doctor query becomes unsafe-host-path without migration or repinning. The runtime protocol tests keep mutation receipt lookup/capacity independent across V1.2/V1.3 and prove repeated, ordered, concurrent live queries neither replay nor consume receipts. The adapter contract tampers a ledger name at constant row count, proves read-only live revalidation reports it as incompatible, restores the ledger, and revalidates successfully; a terminal direct-adapter fixture removes `pgcrypto` in its own disposable database and proves the missing extension is incompatible without restoration. The harness also exercises an in-flight Rust BlobSession across an immediate PostgreSQL restart: the old session loses its hash lock and transaction-B connection, its stale claim cannot complete, and a reassigned exact retry verifies already-published bytes before committing metadata. It also proves `setval` denial, same-signature callable hostile-`search_path` safety, Turkish ICU credential behavior, and populated dump/restore. The XY-1264 proof additionally exercises rollback, blob, and cache behavior (`crates/decodex-postgres/src/socket.rs`, `crates/decodex-runtime/tests/bootstrap_doctor.rs`, `scripts/vnext/postgres_store_test.py`, `spikes/vnext-storage/proof.py`, `spikes/vnext-storage/README.md`).

The V10 extension raised the closed production inventory to 80 functions, 59 non-internal
triggers, and 18 security definers. The two additional definers are the command-complete
RuntimeSession creation and transition owners; the other three new private/builder routines and
three new trigger routines are security invokers. Runtime receives EXECUTE only on the two command
owners and SELECT-only access to RuntimeSessions and their profile/account snapshots. The
additional privileged-function fixture is therefore the eighty-first function at V10. The frozen
XY-1337 fixtures cover exact request substitution and replay, stable rejection replay, profile and
duplicate races, hostile DML/helper/audit namespaces, five atomic rollback boundaries, V9-to-V10
identity-preserving upgrade, and populated restore. The manager-owned final PostgreSQL 18 run adds
clean V1-to-V10 bootstrap, classified zero-state and blocked-old-writer cutover, whole-transaction
retry, crash/restart, dump/restore, and stress schedules; those live gates are deliberately not run
during candidate construction.

V11 adds canonical PostgreSQL WorkItems without introducing execution ownership. The closed
inventory is 98 functions, 69 non-internal triggers, and 23 security definers. Runtime can execute
four exact WorkItem command owners plus one inert future running/resume guard, and receives
SELECT-only access to five normalized WorkItem relations. Readiness re-reads and locks the current
WorkItem, Project, canonical Lead, Program/Objectives, dependency/blocker graph, lifecycle, and
revision state in one transaction before recording typed blockers or entering `ready`. Lead
acceptance snapshots exact-review-revision criteria, evidence provenance, and database chronology
without changing lifecycle or revision; completion remains unowned. The focused V11 harness mode
covers exact replay and concurrent convergence, cycle rollback, readiness blocker persistence,
guard inertness, acceptance immutability and non-completion, clean V1-to-V11 bootstrap, and populated
dump/restore.

V12 adds only inert blocked/waiting ManagedRuns and one safety consumer transaction. At the
historical V12 boundary, the closed inventory was 107 functions, 84 non-internal triggers, and 24
security definers. Runtime receives
SELECT-only access to six ManagedRun/effect/readback relations and EXECUTE on one command-complete
safety entrypoint; it has no ManagedRun creation, acquisition, activation, progress, completion,
assignment, submitted-receipt production, or effect-lineage writer authority. Task and Reviewer
assignments bind exact RuntimeSessions and cannot encode Advisor, Lead, or durable Agent identity.
The barrier has only fail-closed `guarded` and `closed` states. V26 removes the drained V12 local
barrier authority and adds execution-coordinator readback. One `managed_run_v26_suite` stage owns
the focused ManagedRun mode and the normal PostgreSQL aggregate. The stage owns its database,
migration, runtime provisioning, baseline capture, V26 behavior, post-behavior capture, dump,
restore, restored capture, and restored behavior. Exact nextest selection must fail when it selects
zero tests. Final aggregate evidence depends on this stage. Source parsing, copied predicate
inventories, regular-expression control-flow proofs, and AST reachability proofs are not part of
this validation boundary.
The final schema produced by every migration version must be a PostgreSQL 18 dump/restore fixed
point so the one exact full-manifest digest remains identical before and after logical restore.
Cross-database manifest identity is semantic rather than catalog-local: every relation, column,
constraint, function signature, trigger, dependency, ACL/default ACL, and sequence is keyed by
stable schema/name/owner-column/principal tuples. Catalog OIDs and presentation renderers are join
mechanics only and never enter emitted identity or ordering. Sequence definitions and stable
ownership belong to the schema manifest; mutable sequence values are a separate restore-state
receipt. Every manifest checkpoint binds an explicit requested database to both configured URLs
and both observed `current_database()` values, while that binding evidence is excluded from
cross-database digest equality. The canonical harness collects source, post-command, populated
RoleProfile restore, and final primary restore evidence before one terminal report.

XY-1353 reset the earlier catalog-presentation model after two canonical-boundary falsifiers: the
normalized source schema changed from the stale `5b546036...` digest to `79fc7a15...`, then the
same committed candidate restored as `b3984125...`. Those failures reject the identity model, not
PostgreSQL logical restore, and prohibit repairing another individual OID or mechanically rebinding
a digest before semantic source/restore parity is established.

The XY-1345 command is a separate non-production architecture proof. It requires exactly
PostgreSQL 18.4, creates a private temporary cluster with TCP disabled, installs only fixture
roles/objects, exercises the deterministic and 50-by-32 stress schedules, performs populated
`pg_dump`/`pg_restore`, stops the cluster, and removes its temporary root on success or failure. Its
JSON output is the command receipt; any `FAILED` result or nonzero anomaly count falsifies the gate.
It does not run the production migration ledger and cannot substitute for the V9 clean-bootstrap,
V8-to-V9 upgrade, authority-manifest, hostile SQL, crash/recovery, focused concurrency/idempotency,
and populated-restore tests owned by XY-1346. The canonical final PostgreSQL harness runs the V9
upgrade, concurrency, crash/recovery, and populated-restore scenarios in isolated databases; it
also injects admin-only receipt/domain/activity/outbox aborts and database-executed
serialization/deadlock failures to prove whole-transaction rollback, retry, and convergence through
the production RoleProfile API. The V9 proof runs once only after the exact candidate is frozen;
active implementation uses targeted Rust compilation, parser/unit contracts, and migration/protocol
syntax checks. See
[the durable evidence page](../evidence/xy-1345-exact-command-authority.md).

The PostgreSQL integration harness bootstraps the shipped V1-V12 migration history, from the `V1`
foundation through the forward-only `V12` ManagedRun safety authority, and verifies
transaction/idempotency/revision behavior, Conversation-lock serialization with append-only history-derived
positions, snapshot high-water, and immutable item-version sequence with no writable stored next-position counter, page-only opaque
issued-cursor pagination with never-issued/expired/cross-Conversation/edited-chain rejection,
fixed chain page size, 512-per-Conversation/4,096-global durable limits, serialized concurrent
capacity, exact-boundary expired-chain pruning, runtime direct-root denial, and the canonical
receipt-before-statement-level-hierarchy/cursor/row lock order under same- and cross-Conversation
history-versus-Artifact races, mutation-stable snapshot replay, canonical
insert-time lifecycle timestamps, immutable RuntimeSession Codex-thread and
last-known-turn correlations across lifecycle transitions, scoped foreign-key and terminal-state
counterexamples, contiguous Artifact revision history and exact parent/current-revision coherence,
receipt-first fenced claims, exact stored-response replay, and cross-operation/entity conflict before
effects, large history and Context Pack blob offload, canonical media-type rejection before authority commit,
missing/tampered/retried direct and transitive blob behavior, sorted session hash/per-shard admission
locks, concurrent shard-capacity enforcement, bounded grace-aged resumable orphan reclamation with
metadata-commit-before-byte-removal crash ordering, two-connection parent/child serialization races,
writer/reclaimer exclusion, complete Context Pack provenance/readback/determinism/truncation, sealed
source-manifest append/update/delete/gap rejection, a real PostgreSQL-backed WebSocket history path,
and hard-disabled transition dispatch. A hostile-search fixture supplies same-signature callable
shadows and proves canonical media validation and trigger timestamps still use catalog semantics.
It also verifies collation-independent credential rejection in
a Turkish ICU database, dumps the populated primary database, restores it into a fresh
database, and reruns the restored contract. The primary contract also exercises
caller-shifted lease/retry/retention anchors, early and due delivered-row deletion, and
forbidden outbox truncation. Intermediate schemas from unshipped branches are not
compatibility targets.

## Managed repository frozen-tree validation

This section describes the managed-repository and historical V22 frozen-tree boundary. It does not
define or satisfy the retired private-artifact unified validation protocol.

Managed-repository stage-two work has no pre-freeze execution gate. Do not run compile, test,
check, Clippy, format, migration, wrapper, matrix, doctest, behavioral, app, or benchmark commands
while its parallel owners construct the candidate. After serial integration freezes one exact
tree, run one complete unified validation on that tree only. Keep the resulting evidence categories
concise: pure semantics; PostgreSQL authority, concurrency, restore, and retention; accepted
Git/filesystem execution and operation-specific readback; first shared-saga composition; provider
and repository integration; and final digest/manifest agreement. Partial runs, expanded early test
matrices, and results from any other tree are not acceptance evidence.

The managed-repository deferred cases are the
[XY-1353 deferred acceptance matrix](../specs/vnext-gates.md#xy-1353-deferred-acceptance-matrix).
For the V22 retained-title bridge, the
[XY-1367 V22 deferred acceptance matrix](../specs/vnext-gates.md#xy-1367-v22-deferred-acceptance-matrix)
is historical acceptance context. It does not grant current command authority or define the
future package validation protocol.
The repository has no standalone XY-1353 artifact generator: migration, configured-authority, and
schema inventories plus their expected/actual digests are emitted and checked by the canonical
PostgreSQL frozen-tree harness during that one unified gate. Do not run it as integration-time
code generation or create parallel manifest authority.

## Manual V22 retained-title experiment

The manual runner is not a validation command. Run it only under separate live-effect authority.
Do not run it during XY-1367 or XY-1368.

The runner requires the `retained-title-experiment` feature. No default feature enables it.
The `decodexd` application does not depend on this feature or binary.

```sh
cargo run -p decodex-runtime \
  --features retained-title-experiment \
  --bin decodex-retained-title-experiment -- REQUEST.json
```

`REQUEST.json` has one closed JSON object. It contains these fields:

- `identity`: The complete V14 experiment identity.
- `creation_attempt_id`: The UUID for the one start fence.
- `title_attempt_id`: The UUID for the one title-set fence.
- `attestation_id`: The UUID for the exact-ID retained-title attestation.
- `observation_id`: The UUID for the positive read observation.
- `timeout_milliseconds`: A value from 1000 through 60000.

The runner fixes the app-server request IDs to 3, 4, and 5. They identify start, title set, and
read. It derives each database idempotency key from the experiment ID and operation name.

The runner first verifies configuration, database readiness, executable identity, schema, and
account identity. It then permits only this effect sequence:

1. Prepare the experiment.
2. Commit the creation fence.
3. Send one `thread/start` request after a fresh fence.
4. Bind the exact start request and response immediately.
5. Commit the title-set fence.
6. Send one `thread/name/set` request after a fresh title fence.
7. Read only the exact bound thread ID.
8. Commit the retained-title attestation.
9. Commit the positive observation.

A creation-fence replay can only read the exact durable start receipt. An absent receipt is
terminally ambiguous. A title-fence replay can only read the exact bound thread.

A lost title-set response can only continue to exact-ID readback. No path retries an external
request. Database transport can retry only the same key and byte-equivalent envelope.

## Validation scope selection

Use the aggregate gate before broad readiness, landing, or release-readiness claims. During iteration, choose the smallest command that covers the touched contract, then name that scope in handoff notes. A narrow validation result is useful evidence, but it is not equivalent to the broad gate unless the change is truly limited to that surface.

Good targeted scopes are contract-shaped rather than file-shaped: CLI parsing/output, runtime state transitions, tracker comments, GitHub status and merge behavior, app-server payloads, site type/build behavior, or plugin/generated-artifact sync. If a change crosses scheduler, review/landing, state, or public/private projection boundaries, start with the relevant focused tests and finish with a broader Rust or repo gate when feasible.

## Owner path source map

Use the owner path to choose the first validation surface:

- `crates/decodex-core/`: vNext domain/application contracts and authority ports plus
  the typed `~/.decodex` root, bounded/redacted config, stable server identity,
  integrity-verifying blobs, and disposable bounded cache. For managed repositories it
  owns only mechanism-neutral values, facts, descriptors, evidence, and pure deciders;
  these are not durable authority.
- `crates/decodex-protocol/`: version, same-UID Unix namespace authority, and bounded
  typed WebSocket client transport shared by CLI and future UI clients.
- `crates/decodex-postgres/`: explicit PostgreSQL product-state adapter and isolated
  real-PostgreSQL integration tests; XY-1307 runtime composition supplies only typed
  explicit configuration and retains unavailable on every bootstrap failure. XY-1349 is
  the sole V13 owner for managed-repository projection, generation/tip, global operation
  assignment, append-only evidence, compare-and-swap, transaction completeness, receipt
  provenance, retention, and restart loads.
- `crates/decodex-codex/`: typed shared-home Codex adapter foundation; live dispatch is
  disabled in current source. Slice 1 may enable only fenced initial fixed/balanced
  selection; XY-1304 remains the later automatic fallback/wake gate.
- `crates/decodex-runtime/`: `decodexd` lifecycle assembly over the four narrow owners;
  for managed repositories it sequences accepted owners but creates no state or receipt
  authority. XY-1351 owns the first shared saga path.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/`: active vNext composition roots.
- `apps/decodex/`: frozen v0.2 source excluded from the workspace; provenance only.
- `apps/radar/`: Radar auxiliary tool for upstream evidence, release deltas, signal rendering, artifact validation, and ledger workflows.
- `apps/decodex-publisher/`: Publisher auxiliary tool for social candidate, reservation, post validation, and publication handoff workflows.
- `plugins/decodex/`: installable Decodex runtime/operator plugin source, including planning, runtime ops, commit, and landing skills/hooks.
- `automations/radar/` and `automations/decodex/`: repo-local Codex App automation sources; generated Radar and Publisher artifacts stay under `.agent/automations/**/cache`.
- `site/`: Astro/TypeScript public static site and app download entry; validate with site type/build commands rather than runtime checks.
- `apps/decodex-app/`: current native SwiftPM macOS account UI. It contains one
  daemon-protocol account surface under
  [Account Lifecycle Authority](../specs/account-lifecycle-authority.md), with no
  local account pool, helper/server workflow, or dual authority.
- `spikes/vnext-storage/`: isolated XY-1264 PostgreSQL, blob, and bounded-cache feasibility proof; validate it with `cargo make test-vnext-storage-proof` and use [the evidence record](../evidence/vnext-storage-feasibility.md) for accepted choices and boundaries.
- `scripts/`: repository helpers; `scripts/assets/` owns checked-in asset generation,
  and `scripts/macos/` owns macOS app packaging checks and the source-install local
  service installer.
- `.github/`: repository automation such as CodeQL code scanning ruleset support.

## Targeted Rust checks

Common targeted commands:

```sh
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo make test-vnext-architecture
cargo test -p decodex-core --all-targets --all-features
cargo test -p decodex-core -p decodex-protocol -p decodex-postgres -p decodex-codex -p decodex-runtime
cargo test -p radar <filter>
cargo test -p decodex-publisher <filter>
```

The remaining test map on this page describes frozen v0.2 provenance and remains useful
only when later removal work audits preserved behavior. It is not an active vNext test
surface.

## Test map

Use source placement over stale historical test counts; current high-value areas are:

- `apps/decodex/src/orchestrator/tests/`: intake, retry, review/landing, runtime cleanup, operator status, repo gates, Program dispatch, reconciliation.
- `apps/decodex/src/agent/tracker_tool_bridge/tests/`: dynamic tracker tools, continuation guards, review handoff, review repair, closeout, terminal finalize, public/private projections.
- `apps/decodex/src/agent/app_server/tests/` and `apps/decodex/src/agent/json_rpc/tests/`: app-server JSON-RPC parsing, dynamic tools, phase goals, thread/turn runtime, transport failures.
- `apps/decodex/src/state/tests/`: SQLite persistence, leases, run-control channels, protocol replay, schema migrations, runtime records.
- `apps/decodex/src/cli/tests/`: CLI parsing and command contract checks.
- `apps/decodex/src/mcp/tests/`: MCP resources, HTTP transport, CORS/auth, capability profiles, lane/project control tools.
- `apps/decodex/src/config/tests/` and `apps/decodex/src/workflow/tests/`: config and workflow policy parsing.
- `apps/decodex/src/manual/tests/`, `apps/decodex/src/github/tests/`, `apps/decodex/src/worktree/tests/`, `apps/decodex/src/recovery/tests/`: Git, PR, landing, worktree, and recovery helpers.
- `tests/scripts/test_sync_installable_plugins.py`: Python test for installable plugin sync and repo-local global skill cleanup.

When adding tests, protect an externally visible contract: CLI output, status JSON, tracker comments, runtime DB state, app-server protocol payloads, Git commands, or public/private boundary behavior. Behavior families that deserve focused tests include scheduler/intake/retry transitions, review and landing classification, tracker mutation writebacks, app-server JSON-RPC payloads, SQLite state and leases, MCP resources/tools, config/workflow parsing, recovery/retained-worktree flows, and Radar/Publisher artifact validation. Prefer table-driven cases when inputs vary only by spelling or equivalent invalid values; keep separate tests when the state-machine outcome, persisted lifecycle marker, authority boundary, process boundary, or observable public surface differs.

Non-Rust validation matters when the touched surface is not in the Cargo workspace: use `npm --prefix site run check` or `npm --prefix site run build` for the Astro site, the plugin and automation Python commands for generated installable artifacts, and the Swift/macOS staging commands for `apps/decodex-app/`. Do not treat `cargo test` as coverage for those surfaces.

## CLI and operator command discovery

The active vNext CLI starts in `apps/decodex-cli/src/lib.rs`. Use these commands for
the shared reset-card service:

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

`account profile` uses the fixed daemon-owned ChatGPT profile endpoint. It is independent
from Reset Card inventory. The default JSON result reports
`email.visibility = "redacted"`. `--include-email` permits one bounded current credential
email in that response. Current and cached results carry one profile snapshot. An
unavailable result carries one typed error, the same explicit email visibility, and an
optional current credential `plan_type` claim. PostgreSQL stores only the latest non-secret
profile snapshot and at most 36 daily usage facts. It stores no email, plan claim, token,
or provider body.

`fast-mode status` reads the canonical Codex `[features].fast_mode` Boolean.
`fast-mode set --enabled BOOL` atomically replaces `~/.codex/config.toml` after changing
only that key. It preserves unrelated TOML, creates no backup, and introduces no second
configuration authority.

Create and persist the key before `use`; the CLI does not generate it. The account registry and
reset-card inventory responses retain the selected profile and stable server authority. Retain
that authority when a selection or pending operation crosses commands:

```sh
decodex \
  --profile NAME \
  --expected-server-id SERVER_UUID \
  --output json \
  reset-card list --account ACCOUNT_UUID
```

Use the same two global options for `use` and `status`. Reset-card commands reject a
remote profile before connection. Current remote profile data is not an authenticated
mutation transport.

Each `use` invocation sends the consume command at most once and then polls durable
status. If output remains `prepared`, use `status`; do not submit a new key for the same
selected card. A recovery client may invoke `use` again only after `status` reports
`not_found`, and it must reuse the exact persisted request and key. Add `--output json`
for the stable `decodex/reset-card-cli/1` projection.
After key creation, every `use` JSON result repeats `idempotency_key` and includes one
closed `dispatch_state`:

- `definitely_not_dispatched`: The CLI did not attempt the protocol send. Retain the key.
- `potentially_dispatched`: The send may have reached the daemon. Query `status` with the
  same key before a same-key resume.
- `durably_accepted`: The daemon returned a durable accepted operation state.
- `rejected_before_acceptance`: The daemon rejected the command before durable
  acceptance.

The Rust service and CLI run on supported macOS and Linux hosts. The native SwiftUI
client is the only macOS-specific reset-card surface.

Run the focused durable reset-card store proof after changes to preparation, effect
fencing, reconciliation, retention, or recovery:

```sh
python3 scripts/vnext/postgres_store_test.py --focus-reset-cards
```

The excluded v0.2 runtime command surface starts in `apps/decodex/src/cli.rs`. For its
preserved command details, prefer:

```sh
decodex --help
decodex <subcommand> --help
```

Important source modules:

- `apps/decodex/src/cli/control_commands/run.rs`: `decodex run`.
- `apps/decodex/src/cli/control_commands/serve.rs`: `decodex serve`.
- `apps/decodex/src/cli/control_commands/status.rs`: `decodex status`.
- `apps/decodex/src/cli/control_commands/lane.rs`: lane inspect/steer/interrupt.
- `apps/decodex/src/cli/control_commands/project.rs`: project registry.
- `apps/decodex/src/cli/control_commands/mcp.rs`: MCP gateway.
- `apps/decodex/src/cli/research_intake_commands/intake.rs`: Program Intake.
- `apps/decodex/src/cli/recovery_commands.rs`: recovery command families.
- `apps/decodex/src/cli/manual_commands.rs`: commit and land.
- `apps/decodex/src/cli/account_commands.rs`: account pool.
- `apps/decodex/src/cli/verify_commands.rs`: validation status publishing.

## Local validation status gate

Frozen v0.2 projects choose `[github].landing_mode` in their `project.toml`. The current
`decodex.example.toml` is the vNext global path/config template and no longer models this
frozen project setting.
The default `standard` mode waits for GitHub's status rollup and ordinary merge
gates. `fast` mode trusts the Decodex local full-check status
`decodex/local-full-check`, requires `landing_actors`, and allows those actors to
execute ruleset bypass landing after local validation passes. The publish command
attaches the local validation status to the exact PR head and base evidence
(`apps/decodex/src/cli/verify_commands.rs`):

```sh
cargo make check
HEAD_SHA="$(git rev-parse HEAD)"
BASE_REF=main
git fetch origin "$BASE_REF"
BASE_SHA="$(git rev-parse "origin/$BASE_REF")"
decodex verify publish-status \
  --config /path/to/project.toml \
  --pr https://github.com/OWNER/REPO/pull/NUMBER \
  --context decodex/local-full-check \
  --state success \
  --expected-head "$HEAD_SHA" \
  --expected-base-ref "$BASE_REF" \
  --expected-base-oid "$BASE_SHA" \
  --description "cargo make check passed"
```

Success requires head/base preconditions, preventing stale green statuses after PR or target branch movement. Publish only after the cited command has passed on the exact tree, and include a description that lets a later operator connect the GitHub status back to the local evidence packet. In fast landing mode, the local status is a merge authority boundary, so a moved PR head, moved base branch, wrong context, or unapproved status creator should stop landing rather than be worked around manually.

## Code scanning

GitHub rulesets for this repository require CodeQL code scanning before merge.
The checked-in workflow is `.github/workflows/codeql.yml` and runs on pushes to
`main`, pull requests targeting `main`, and a weekly schedule. It
analyzes Rust and JavaScript/TypeScript with no-build CodeQL mode so the
required code-scanning tool is configured for PR heads without adding a second
repository build gate.

## App-server compatibility checks

For app-server integration work:

```sh
codex app-server generate-json-schema --experimental --out target/decodex-app-server-schema-check
cargo test -p decodex-codex --all-targets --all-features
cargo test -p decodex-runtime macos_attested_spawn --lib
cargo test -p decodex-runtime live_read_only_probe_negotiates_without_dispatch -- --ignored
```

Runtime's private supervisor validates the accepted receipt, captures and protects the exact
executable snapshot, then structurally validates canonical generated-schema digests before
app-server spawn. Linux executes the sealed snapshot. macOS runs preflights from the immutable
snapshot, then starts the canonical final image suspended and checks its full source digest,
snapshot-rooted CDHash, dynamic path, session, and process group before resume. Private FIFO
endpoints are atomic close-on-exec and have no live names before user code starts. This canonical
macOS image preserves process-attributed network-extension routing without moving Reset Card
ownership into Swift. The child receives only the fixed `HOME`, system `PATH`, and
`CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` projection. The Codex adapter owns the typed
schema/capability contracts but exposes no launch surface.
Markers are not capability promises. Focused tests cover the golden, exact-build cache
conflicts, scripted fake server, structural history/collaboration-schema rejection, fixed
production command construction, bounded executable/preflight/schema/frame/queue/result
inputs, timeout and descendant/orphan cleanup, typed or hashed untrusted event strings,
shared-home/account re-attestation, redacted debug surfaces, and default-disabled dispatch.
The accepted marker now binds Codex CLI `0.146.0-alpha.9.2` to
`ClientRequest.json` digest
`6ffc593d603d21a051840539a4dbfad95cad2e7fec315e252b6722bd71bf37b4`,
`ServerNotification.json` digest
`abbb54060ea6a6005e63267bc6996eacd70cbb7954a7e0d61f50ea02af4acf02`, and
aggregate v2 schema digest
`e554a74bd59d38d16acb1744750b2999156ee3d65d0fe906b22ab52edf17fbbc`.
It also pins `ServerRequest.json`, `v2/LoginAccountParams.json`, both root refresh schemas,
`v2/ThreadReadResponse.json`, and `v2/ThreadStartParams.json` to the digests in the
[exact image receipt](../evidence/xy-1422-codex-0146-account-callback.md).
Codex CLI release `rust-v0.145.0` is rejected as an obsolete exact build. Its
`ClientRequest.json`, `ServerNotification.json`, and aggregate v2 schema digests are
`92085c18742dd355e5afa7d570170c74629635082e8e3341a952068735dc28b2`,
`97c6bf194b9edfa1e2ffe62547e4497fa5ea8a1af5c94687956b69966ac6f9e2`, and
`27f8d983f19d8e1a5548d52176de0a460fb05aaf2a72110f913c6f4af2bd4f27`.
Codex CLI prerelease `rust-v0.146.0-alpha.11` is also rejected as an incompatible exact
build. Its three digests are
`6755a5eb5fcc0502a9d3b56c8ebd43a857f2c22820cf9cfa12e2dd0d5d48234c`,
`28fd2f3e9020a1a26503facff7038f84137c2a0139df8443eab6d63a71deb240`, and
`5ff4540622e002308ad5e6bac6df49b7ab5d52d79c8f71537b1098951b946b2d`.
No compatibility shim for either release is retained. The previously accepted
three-digest set is also rejected as an incompatible build.
The ignored live test is strictly read-only: `initialize`, `initialized`, `account/read`, and
bounded `thread/list(useStateDbOnly=true)` with a fixed nonmatching search term. It sends optional
exact-ID `thread/read(includeTurns=false)` only if the filtered list unexpectedly returns a thread.
The exact `alpha.9.2` schema advertises `thread/search`, but the method did not return during the
bounded live check. The test does not invoke it and records it as `not_probed`. These probes do not
establish global title discovery. Do not replace the live test with excluded v0.2
`decodex probe stdio://`, which starts a proof turn.

## Plugin and automation checks

Installable plugin sync:

```sh
python3 scripts/config/sync_installable_plugins.py
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
python3 -m unittest tests/scripts/test_sync_installable_plugins.py
```

The Decodex plugin manifest declares runtime package include/exclude patterns.
`scripts/config/sync_installable_plugins.py` must honor that contract when
installing to `$CODEX_HOME/plugins/cache/hack-ink/decodex/<version>`, so
source-only plugin tests are not copied into installed packages.

Codex App automation rendering and evaluation:

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

`automations/portfolio.toml` declares exactly three upstream roles and two content
roles. The renderer and evaluator cannot write scheduler state. Apply definitions only
with the native Codex automation lifecycle tool, then read back every field. Codex App
owns machine-local timestamps.

## Static site checks

The site is an Astro/TypeScript static surface (`site/package.json`). Commands:

```sh
npm --prefix site install
npm --prefix site run check
npm --prefix site run build
npm --prefix site run dev
```

Use `site/README.md`, `site/src/`, `site/package.json`, and `openwiki/integrations/plugins-automations-and-auxiliary-tools.md` for the current static-site boundary and validation commands.

## Native macOS app checks

The app is outside the Cargo workspace (`Cargo.toml`). Commands from
`apps/decodex-app/README.md`:

```sh
swift build --package-path apps/decodex-app -c release
swift test --package-path apps/decodex-app -c release
apps/decodex-app/script/build_and_run.sh
scripts/macos/test_decodex_app_stage.sh
python3 -m unittest tests.scripts.test_install_decodex_local_service
```

The Swift suite covers one all-account list, UUID-keyed progressive Reset Card rows,
strict native-client decoding, second-click confirmation, bounded startup recovery,
and durable use replay. The staged package contains only the Swift app, in-process Rust
client library, and resources. It contains no CLI, daemon, legacy `decodex`, app helper,
loopback server, `:8192` client, account-pool reader, or migration tool. The separately
installed and signed local service owns the daemon wrapper and PostgreSQL generation.

The local-service installer test starts from a fresh empty product database and verifies
credential-negative config, the signed wrapper, one `supervise-local` LaunchAgent,
doctor readiness, and an empty account list. The installer never reads, copies, or
deletes an old account pool. Local credentials are imported afterward with the ordinary
public account command, followed by one restart for exact-build callback attestation.

## Radar and Publisher checks

Radar:

```sh
radar --help
radar validate .agent/automations/radar/cache/site-content/signals
cargo test -p radar
```

Publisher:

```sh
decodex-publisher validate-social
cargo test -p decodex-publisher
```

Generated Radar artifacts belong under `.agent/automations/radar/cache`; generated Publisher social artifacts belong under `.agent/automations/decodex/cache/social` (`automations/radar/README.md`, `automations/decodex/README.md`).

## Practical change checklist

- CLI option or parsing change: add/adjust `apps/decodex/src/cli/tests/**` and run `cargo test -p decodex cli::tests` or relevant filter.
- Runtime scheduler/lifecycle change: run targeted orchestrator tests, then `cargo nextest run -p decodex` if feasible.
- State schema change: add migration/schema tests and run state tests.
- Public projection change: test redaction and public/private split.
- Plugin hook or installer change: run Python plugin sync tests.
- Site/App change: run the site or Swift checks, not only Rust checks.

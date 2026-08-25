---
type: "Reference"
title: "Private-artifact operations and delivery (retired design)"
openwiki_generated: true
---

# Private-artifact operations and delivery (retired design)

Status: frozen historical, non-executable design evidence.

At and after the [repository effective point](decision.md#repository-effective-point),
every rule marker, owner, delivery edge, issue phase, command, receipt, validation
step, and modal verb in this file describes the retired private-artifact design
only. A0, A1, B, D0a, C, D, CORE-FREEZE, ACC, preparation, and unified validation
are historical and non-executable. They are not active work, deferred work,
dependencies, command authority, or future vNext obligations. Do not create, run,
or restore a task or implementation from this record.

Before the effective point, the fail-closed conditions in the retirement decision
apply and no private-artifact work can start.

## Frozen historical operations and delivery design

### Deterministic controllers

<a id="rule-PA-OPS-0001"></a>
**[rule:PA-OPS-0001]** Durable rows are authority. Controller cursors and claims
are bounded in-memory scheduling state only. Restart begins from the minimum key.

| Controller | Exact order and cap |
| --- | --- |
| Startup | `(class,updated_at,operation_id)`, where class 1 is retained active attempt and class 2 is other nonterminal work; fetch 257 and process at most 256 |
| Preparation scan | `(next_reconcile_at,receipt_id)` over due nonsticky rows; fetch 65, examine at most 64, and select at most one completion turn |
| GC | `(work_class,due_at,subject_variant,subject_bytes,candidate_id)`; fetch 257 and process at most 256 |

A blocked row becomes sticky, moves to its exact future time, or is advanced past.
It cannot hold a cursor. One end-of-keyspace wrap handles new lower keys.
Automatic wakes are only unfinished keyset continuation, the earliest nonsticky due
time, and an eligible second GC observation. Durable new state and explicit
operator action are external wakes. A full wrap with only sticky or terminal work
clears automatic timers. The queue contains at most one coalesced wake.

### Status and readiness

<a id="rule-PA-OPS-0002"></a>
**[rule:PA-OPS-0002]** Startup readiness remains unavailable until one full
bounded wrap classifies all private-artifact rows. Status reads at most 65 rows,
returns at most 64, and reports truncation without dropping authority. Read-only
status and doctor expose only closed types, bounded counters, and redacted
identifiers. Sticky attention, incompatibility, dependency, preparation, GC, and
producer residuals remain visible and do not hot-loop.

The protocol remains compatible with V1.2 through V1.3. Its query and response
shapes are exact:

```text
GetPrivateArtifactStatus { operation_id: Option<UUID> }
PrivateArtifactStatusV1 {
  readiness: Ready | Unavailable,
  counters: [12 fixed CounterV1 values],
  rows: Vec<OperatorProjectionV1>,
  truncated: bool
}
CounterV1 { kind: closed u8, value: u32, saturated: bool }
```

The 12 counter kinds, in fixed order, are `Preparing`, `Running`, `Waiting`,
`Attention`, `AttemptExhausted`, `Incompatible`, `PendingPreparation`,
`ReconciliationResidual`, `GcOrphanResidual`, `RetainedResidual`,
`MaintenanceUnavailable`, and `TotalBlocked`. Counters saturate at `u32::MAX` and
set `saturated`. Rows sort by severity 1 through 7, `blocked_at`, and raw
`status_row_id` UUID bytes. The query uses SQL `LIMIT 65`, returns the first 64
rows, and sets `truncated` if row 65 exists. The authority indexes are:

```text
status_rows_order_idx(severity, blocked_at, status_row_id)
status_rows_operation_idx(operation_id, severity, blocked_at, status_row_id)
prepare_receipts_pending_idx(state, next_reconcile_at, receipt_id)
blob_gc_candidates_work_idx(state, first_observed_at, candidate_id)
private_artifact_operations_startup_idx(lifecycle, updated_at, operation_id)
private_artifact_dependencies_pending_idx(state, operation_id, dependency_kind)
```

Status uses one read-only `READ COMMITTED` transaction with a 2-second statement
timeout, a 100-millisecond lock timeout, and a 3-second wall deadline. Failure
returns `Unavailable`; it never returns partial rows or counters. Doctor issues
are exactly `ExecutorBusy`, `ExecutorGuardUnavailable`, `BootScopeUnavailable`,
`BootScopeChanged`, `MaintenanceUnavailable`, `ReconciliationResidual`,
`ArtifactIntegrity`, `ArtifactIncompatible`, and `AttemptExhausted`. The only new
operator command is read-only:

```text
decodex private-artifact status [--operation UUID]
```

There is no mutating private-artifact command.

### Resource ceilings

<a id="rule-PA-OPS-0003"></a>
**[rule:PA-OPS-0003]** `authority/inventories.json#/resource_ceilings` owns the
exact inclusive limits and calculations. Important derived maxima are 2,060 steps,
6,180 attempts, 24,720 observations, ordinary event ordinal 24,721, hard event
ordinal 24,753, 1,027 simultaneous debts, 74,109,517 bytes of retained evidence,
197,784 ordinary transition protocol statements, 96 calculated private-artifact
handles below a 128-handle cap, and one coalesced wake.

Pre-operation bound failure commits one compact terminal rejection. An operation
bound consumes a reserved event and enters Attention. Handle, SQL, and controller
bounds authorize no effect. No cap truncates an authoritative record. Controller
deadlines stop only the next syscall, handle open, or SQL statement. They cannot
interrupt a syscall already in the kernel.

### Product residual risks

<a id="rule-PA-OPS-0004"></a>
**[rule:PA-OPS-0004]** `authority/inventories.json#/product_residual_risks` is the
closed ten-record product risk set. No risk authorizes fallback, adoption,
override, compatibility mode, alternate persistence, an extra daemon, pre-freeze
test ownership, or a weaker authority rule.

### Authority-package sequence

<a id="rule-PA-DEL-0001"></a>
**[rule:PA-DEL-0001]** Delivery order is exact:

```text
XY-1372 -> AR-PKG -> AR-CUT -> AR-CLOSE -> {A0,D0a}
A0 -> A1 -> B
{B,D0a} -> C -> D
D -> {XY-1369,XY-1370}
{XY-1369,XY-1370} -> XY-1363
XY-1363 -> CORE-FREEZE -> ACC
ACC -> one mechanical preparation pass
one mechanical preparation pass -> unified complete validation protocol
```

AR-PKG owns only this fixed package. AR-CUT stacks directly on its exact frozen
identity and owns only these seven projection paths:

- `openwiki/decisions/vnext-authority.md`
- `openwiki/decisions/sqlite-local-product.md`
- `openwiki/specs/vnext-authority.md`
- `openwiki/specs/vnext-gates.md`
- `openwiki/architecture/runtime-architecture.md`
- `openwiki/operations/commands-and-validation.md`
- `openwiki/quickstart.md`

AR-CUT linked and summarized; it did not duplicate inventories, create rules, edit
this package, or edit the XY-1368 runbook. AR-CLOSE is the one public
package-native authority amendment that accepts signed C2, records the historical
corpus quarantine and semantic-fidelity residual, retires private semantic
rereview, and rebinds the affected projections. Acceptance of this amendment
satisfies AR-CLOSE. A0 and D0a can then begin against its exact signed identity.
All downstream A1/B/C/D/CORE-FREEZE/ACC/preparation/validation edges remain
unchanged.

### Pre-freeze source ownership

<a id="rule-PA-OWN-0001"></a>
**[rule:PA-OWN-0001]** A0, A1, B, C, D, XY-1369, XY-1370, and XY-1363 are
pre-freeze source-writing phases. They can inspect authority and source and use
ordinary read-only Git/fingerprint tools. They cannot edit or execute tests,
fixtures, wrappers, task-runner files, or runbooks; run a build, compiler,
formatter, migration, service, matrix, or validation; create a differential
fixture; or claim execution evidence. Their receipts prove scope, identity, and
reviewed intent only. D0a is the sole exception.

Exact production ownership is:

- A0 restores `crates/decodex-core/src/lib.rs` and
  `crates/decodex-core/src/path_unix.rs` to their accepted blobs and makes
  `path_unix/artifact.rs` and `private_artifact.rs` absent. Only `lib.rs` transfers
  to A1.
- A1 owns `crates/decodex-core/src/lib.rs` and
  `crates/decodex-core/src/private_artifact/{mod,model,codec,reducer}.rs`.
- B owns exactly the eight paths in
  `authority/inventories.json#/issue_path_owners/B`.
- C owns exactly the seven private-artifact runtime module files in
  `authority/inventories.json#/issue_path_owners/C` and can add only its module
  declaration to runtime `lib.rs`, which then transfers to D.
- D owns exactly the composition, admission, runtime, protocol, daemon, and CLI
  paths in `authority/inventories.json#/issue_path_owners/D`.

A1 owns pure values, codec, validity, digests, plans, reducer, and reason codes. B
owns V23, former server store records/transitions, receipts, CAS/reference coordination,
dependencies, debts, retention, GC, ACL, fencing, and the fixed SQL locator. C owns
descriptor acquisition, capture, filesystem effects, repair, sync, guard, boot,
collection, and observation. D owns composition, admission-before-spawn,
supervision, maintenance, restart, controllers, status, V1.3 protocol, doctor,
CLI, and DTO substitution.

No pre-freeze owner changes a Cargo manifest, `Cargo.lock`, root workspace
manifest, `Makefile.toml`, the former server store wrapper, the XY-1368 runbook, or a test or
fixture source. A required dependency, alias, third task, second preparation task,
public command family, or test framework is a stop.

### D0a exception

<a id="rule-PA-OWN-0002"></a>
**[rule:PA-OWN-0002]** D0a owns only
`openwiki/evidence/xy-1371-private-artifact-platform-prerequisites.md`. It owns no
Rust, SQL, migration, manifest, test, fixture, wrapper, runtime, protocol, or CLI
source. The Manager authorizes it as the only pre-freeze critical executable
evidence exception because C cannot safely implement guard, boot, process-group,
namespace, lock-release, and supported-filesystem semantics without exact platform
evidence.

D0a runs once, captures stable complete output, binds exact environment, inputs,
commands, bytes, and hashes, keeps raw receipts private, and commits only bounded
accepted evidence and fingerprints. It runs no repository build, test, wrapper,
formatter, or migration; does not validate B/C/D; does not expand XY-1372; and
does not claim hostile same-UID containment or authorize fallback.

A failed, unavailable, incomplete, mismatched, unstable, or unbound fact blocks C.
D0a has no mechanical retry. A later attempt needs an explicit Manager decision,
a materially changed condition or evidence design, and a new candidate identity.

### CORE-FREEZE

<a id="rule-PA-FREEZE-0001"></a>
**[rule:PA-FREEZE-0001]** CORE-FREEZE begins only after D, XY-1369, XY-1370, and
XY-1363 complete by source inspection; the integrated production and V23 migration
source is present; every path transfer has one final owner; every changed embedded
former server store statement is in the fixed locator; the superseded production
preparation APIs are absent; the one stale ACC-owned test caller is identified;
and no other obsolete caller or test-delegated product invariant exists.

The receipt binds package/cutover and all source-phase receipts, integrated HEAD
and tree, full changed paths, production/migration hashes, complete locator IDs and
source fingerprints, retained V22 SQL membership, the exact stale test caller,
D0a identity, package/projection hashes, clean state, and ACC base. It uses source
inspection and ordinary read-only fingerprint tools and executes nothing.

After freeze, ACC cannot repair production or migration source. Such a repair
creates a new CORE-FREEZE identity and invalidates dependent receipts. A canonical
formatter delta can be recorded later as formatting-only. Any manual semantic or
production repair is a new freeze identity.

### ACC maximum scope and acceptance source

<a id="rule-PA-ACC-0001"></a>
**[rule:PA-ACC-0001]** ACC starts from the exact CORE-FREEZE receipt and can write
at most these 15 paths:

- `crates/decodex-core/tests/private_artifact_codec.rs`
- `crates/decodex-core/tests/private_artifact_reducer.rs`
- `crates/decodex-server-store/tests/server-store_store.rs`
- `crates/decodex-server-store/tests/server-store_store/private_artifacts.rs`
- `crates/decodex-core/tests/private_artifact_blob_gc.rs`
- `scripts/vnext/server-store_store_test.py`
- `tests/scripts/test_server-store_authority_capture.py`
- `crates/decodex-runtime/src/private_artifact/tests.rs`
- `crates/decodex-runtime/tests/private_artifact_integration.rs`
- `crates/decodex-runtime/tests/bootstrap_doctor.rs`
- `crates/decodex-runtime/tests/cli_diagnostics.rs`
- `crates/decodex-runtime/tests/websocket_protocol.rs`
- `tests/scripts/test_vnext_architecture.py`
- `Makefile.toml`
- `openwiki/operations/xy-1368-retained-title-validation.md`

This is a maximum, not a required edit list. The mandatory delivery-retirement
slice is exactly the former server store integration test, former server store wrapper, task runner,
and XY-1368 runbook. An unused optional test path stays unchanged. No Cargo
manifest or lockfile is in scope.

<a id="rule-PA-ACC-0002"></a>
**[rule:PA-ACC-0002]** ACC adds the minimum high-value acceptance source for
architecture, authority, Rust/former server store codec agreement, `Reason(8,6)`, V22
preservation, V23 and ACL, restart/recovery, guard/boot integration without
re-proving D0a, accepted deferred cases, one representative end-to-end flow,
minimum package integrity, integrated retained-title semantics, and one consumer
of the production SQL locator.

For each deferred case, ACC records existing sufficient coverage, a small extension
to an existing test, one necessary new contract test in an allowlisted path,
coverage by the end-to-end flow, or a required production change. A required
production change stops and invalidates CORE-FREEZE. ACC rejects permutations,
duplicate or implementation-detail assertions, test-count targets, new
dependencies/frameworks, new command families, package-specific checkers,
test-only production visibility, and command-patch-command loops.

Minimum semantic groups are model/codec, reducer, former server store authority, GC,
executor, runtime, protocol/CLI, architecture, and one prepare-to-terminal flow.
Package-integrity assertions can check file presence, fixed TSV/JSON shapes, raw
manifest hashes, row order, unique IDs, owner references, and V22 snapshot hashes.
They cannot duplicate semantic constants or decide package acceptance.

### Fixed production SQL locator

<a id="rule-PA-PREP-0001"></a>
**[rule:PA-PREP-0001]** B adds one production-compiled, non-test-only fixed
locator in `private_artifacts.rs`, re-exported through former server store `lib.rs`:

```rust
pub struct VNextPostgresPreparationSourceV1 {
    pub source_id: &'static str,
    pub sql: &'static str,
}

pub fn vnext_server-store_preparation_sources_v1(
) -> &'static [VNextPostgresPreparationSourceV1];
```

The locator contains every embedded former server store statement added or changed from the
accepted base through CORE-FREEZE, including the five retained V22 statements.
Each source occurs once and rows sort lexically by stable
`module::CONST_IDENTIFIER`. `sql` references the production constant and does not
copy SQL bytes. There is no dynamic registry, trait, plug-in, macro framework,
filesystem scan, parser, runtime discovery, I/O, protocol change, or second
inventory.

B removes the superseded production helper and fixture, keeps the five production
SQL constants, and exposes them only as needed by this locator. It adds no async
preparation helper or test-only production export. If a downstream owner changes
one of those constants, it updates the same in-module reference set under an
explicit sequential path transfer before CORE-FREEZE. A later embedded SQL source
outside the locator stops work and returns to B; ACC cannot repair it.

### Distinct canonical former server store tasks

<a id="rule-PA-PREP-0002"></a>
**[rule:PA-PREP-0002]** ACC owns one mechanical task:

```toml
[tasks.check-vnext-server-store-preparation]
workspace = false
command = "python3"
args = [
    "scripts/vnext/server-store_store_test.py",
    "--prepare-vnext-server-store",
]
```

It creates one private former server store 18 cluster, checks and applies the complete exact
V1-V23 ledger, requires terminal migration 23 named
`private_artifact_authority`, provisions runtime, gets sources only from the fixed
locator, calls `Client::prepare` once for every source without executing a prepared
statement, collects all possible source results, emits schema/configured-authority
digests, tears down, and emits one stable report.

The outer schema is `decodex/server-store-preparation-stage-report/1`, mode
`vnext_server-store_preparation`. Inner stages use
`decodex/server-store-preparation-stage/1` and are `cluster_preflight`,
`migration_syntax`, `changed_embedded_sql_prepare`,
`generated_authority_inventory`, `teardown`, and `final_report`. The wrapper must
not copy a source list, assume a count of five, parse Rust to discover SQL, select
only V22 SQL, call a removed fixture, or emit a retained-title preparation
identity. There is no alias, dependency from `cargo make check`, unrelated task
rename, or second preparation task.

<a id="rule-PA-PREP-0003"></a>
**[rule:PA-PREP-0003]** The distinct semantic task remains:

```toml
[tasks.test-vnext-retained-title-core]
workspace = false
command = "python3"
args = [
    "scripts/vnext/server-store_store_test.py",
    "--focus-retained-title-core",
]
```

It is not preparation. It checks the ordered ledger as exactly V1-V23, requires 23
entries, requires terminal V23 `private_artifact_authority`, requires V22
`retained_title_experiment_bridge` in position 22, applies all migrations, and
runs the retained-title semantic boundary against that integrated schema. It keeps
the accepted V22 two-effect protocol and authority assertions.

The inner receipt is `decodex/server-store-retained-title-acceptance/2`. It records
first version 1, last/terminal version 23, count 23, terminal name, retained-title
version 22 and name, ordered-ledger digest, V22 semantic result, authority and
architecture digests, environment/command identity, and final status. The outer
schema is `decodex/server-store-retained-title-stage-report/2`; its mode remains
`retained_title_boundary`.

The mechanical and semantic tasks do not call each other. Sharing one wrapper does
not merge their contracts. Neither task can publish `decodex/local-full-check`.

### Canonical surface-retirement table

<a id="rule-PA-RET-0001"></a>
**[rule:PA-RET-0001]** This table is the only non-callable canonical record of
each obsolete identifier or equivalent contract. Exact identifier matching uses
the complete lexical identifier, task, option, or schema value, not substrings of
another row. Base locations are bound to
`4daf4dd809411bc83d7ea912e6b99612d4c9572a`.

| ID | Obsolete identifier or equivalent | Base occurrence classification | Owner and action | Replacement | Permitted historical evidence |
| --- | --- | --- | --- | --- | --- |
| `RET-01` | `prepare_retained_title_sql` | Callable product definition and delegation at `crates/decodex-server-store/src/experiments.rs:32` and `crates/decodex-server-store/src/lib.rs:221`; remove | B: delete function and sole-purpose visibility/imports | Fixed locator; ACC prepares each locator entry | None |
| `RET-02` | `PostgresStore::prepare_retained_title_sql_fixture` | Callable test-support definition at `crates/decodex-server-store/src/lib.rs:218` and test call at `crates/decodex-server-store/tests/server-store_store.rs:445`; remove | B: delete fixture; ACC replaces its one caller | Direct test consumption of fixed locator | None |
| `RET-03` | `server-store_retained_title_sql_preparation_contract` | Test caller at `crates/decodex-server-store/tests/server-store_store.rs:438` and wrapper selector at `scripts/vnext/server-store_store_test.py:4153`; rewrite | ACC: replace both ends atomically | Integrated locator-driven preparation contract | None |
| `RET-04` | `RETAINED_TITLE_SQL_SOURCES` | Wrapper inventory and consumers at `scripts/vnext/server-store_store_test.py:229,4158-4159`; remove | ACC: delete list and count assumption | Rust locator is sole inventory | None |
| `RET-05` | `RETAINED_TITLE_PREPARATION_DATABASE` | Wrapper database constant and consumers at `scripts/vnext/server-store_store_test.py:138,2203,4122-4132,4149`; rename and generalize | ACC: replace in one wrapper cutover | Generic V1-V23 preparation database identity | None |
| `RET-06` | `prepare_retained_title_migrations` | Wrapper function/call at `scripts/vnext/server-store_store_test.py:4119,6159`; replace | ACC: rewrite for exact integrated ledger | V1-V23 migration preparation stage | None |
| `RET-07` | `prepare_retained_title_embedded_sql` | Wrapper function/call at `scripts/vnext/server-store_store_test.py:4148,6165`; replace | ACC: rewrite for all locator sources | Locator-driven SQL preparation stage | None |
| `RET-08` | `prepare_retained_title_authority_inventory` | Wrapper function/call at `scripts/vnext/server-store_store_test.py:2190,6171`; replace | ACC: generalize authority inventory stage | Generic generated-authority inventory stage | None |
| `RET-09` | `check-vnext-retained-title-preparation` | Task audit/definition at `Makefile.toml:22,61` and command runbook at `openwiki/operations/xy-1368-retained-title-validation.md:10`; remove | ACC: atomic task, wrapper, test, runbook cutover | `check-vnext-server-store-preparation` | None |
| `RET-10` | `--prepare-retained-title-core` | Task argument and wrapper parser/usage at `Makefile.toml:66` and `scripts/vnext/server-store_store_test.py:5874,5900`; remove | ACC: replace parser and task argument without alias | `--prepare-vnext-server-store` | None |
| `RET-11` | `decodex/retained-title-preparation-stage/1` | Current wrapper emission at `scripts/vnext/server-store_store_test.py:4141,4156`; remove | ACC: replace all current executable emissions | `decodex/server-store-preparation-stage/1` | None |
| `RET-12` | `retained_title_preparation` | Current wrapper final-report mode at `scripts/vnext/server-store_store_test.py:7633`; remove | ACC: replace generic preparation mode | `vnext_server-store_preparation` | None |
| `RET-13` | `decodex/server-store-retained-title-acceptance/1` | Current wrapper emission at `scripts/vnext/server-store_store_test.py:2288`; historical evidence at freeze page line 9 | ACC: current executable source uses version 2 | `decodex/server-store-retained-title-acceptance/2` | Only `openwiki/specs/xy-1368-retained-title-freeze.md` as immutable V22 historical evidence |
| `RET-14` | `decodex/server-store-retained-title-stage-report/1` | Current wrapper emission at `scripts/vnext/server-store_store_test.py:7628`; historical evidence at freeze page line 10 | ACC: current executable source uses version 2 | `decodex/server-store-retained-title-stage-report/2` | Only `openwiki/specs/xy-1368-retained-title-freeze.md` as immutable V22 historical evidence |
| `RET-15` | `V14-V22 retained-title core` | Current wrapper acceptance at `scripts/vnext/server-store_store_test.py:2246` and current runbook scope at `openwiki/operations/xy-1368-retained-title-validation.md:3`; rewrite | ACC: state integrated ledger truthfully | Retained-title semantics anchored at V22 on V1-V23 | Freeze page can describe accepted V22 history but is not command authority |
| `RET-16` | V22-terminal ledger predicate | Current wrapper requires 22 entries and terminal migration 22; current runbook repeats the old preparation scope | ACC: require exact 23-entry integrated ledger and explicit V22 semantic position | V23 terminal predicate plus V22 semantic predicate | Freeze page can preserve the historical V22-terminal acceptance fact |
| `RET-17` | Five-source-only preparation capability | Current product helper/fixture, test, wrapper source tuple, count, and runbook describe a five-statement preparation boundary | B removes production surface; ACC removes delivery surface and renamed equivalents | One complete fixed production locator | No callable or current-command historical exception |

### Surface-aware closure and historical classification

<a id="rule-PA-RET-0002"></a>
**[rule:PA-RET-0002]** After ACC authoring and before any command runs, inspect
callable product source, tests, wrappers, task-runner authority, the current
XY-1368 runbook, and all current projections. Every RET-01 through RET-17 surface
must be absent or replaced as its row requires. No renamed equivalent can prepare
only the five V22 statements, expose a five-source fixture, duplicate the five
names in Rust/Python/TOML/Markdown, assume five sources, require a 22-entry current
ledger, treat V22 as current terminal migration, reach an old task through an
alias, or emit an old preparation or semantic receipt in executable source.

This canonical table is non-callable and is the sole package exception for exact
obsolete names. The scan records exact queries, zero-result callable/current
obligations, remaining retained-title occurrences grouped by allowed semantic
context, base/HEAD/tree/paths, and each historical exception. Any unclassified
occurrence stops ACC.

Allowed current retained-title contexts are V22 migration, relation, SQL, protocol,
and product identifiers; experiment runtime and CLI; semantic tests; the distinct
semantic task and mode; the semantic boundary mode; version-2 semantic receipts;
and the retained-title semantic section of the current XY-1368 runbook. A
retained-title name for generic integrated preparation is invalid.

<a id="rule-PA-RET-0003"></a>
**[rule:PA-RET-0003]** Classify
`openwiki/specs/xy-1368-retained-title-freeze.md` as immutable V22 historical
evidence. It is not current command authority, task-runner authority, an executable
receipt producer, or a current V1-V23 ledger contract. Its two historical schema
occurrences are the only permitted version-1 semantic-receipt occurrences after
ACC.

The current command owner is
`openwiki/operations/xy-1368-retained-title-validation.md`. ACC changes it in the
same candidate as the integration-test caller, wrapper, and task runner. The
updated runbook owns the post-freeze generic preparation task, the separate
retained-title semantic task, and the unified gate. It states V1-V23, the complete
locator, no prepared-statement execution, generic preparation receipts, V22 as the
semantic anchor, version-2 semantic receipts, and no full-check publication by a
focused task.

Other checked-in retained-title occurrences on the base are current product or
projection terminology, not historical command exceptions. AR-CUT classifies
projection occurrences as nonnormative summaries. ACC classifies every remaining
callable, test, wrapper, task, and runbook occurrence. No occurrence can remain
unclassified.

### Mechanical preparation

<a id="rule-PA-VAL-0001"></a>
**[rule:PA-VAL-0001]** Mechanical preparation begins only after ACC freezes the
exact CORE-FREEZE-plus-ACC candidate. From its absolute Git top level, use the
fixed nonsecret prefix
`SCCACHE_DISABLE=1 DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`
and run, in order, `cargo make fmt`, `cargo make check-rust`, and
`cargo make check-vnext-server-store-preparation`. The separate initial aggregate is
then `cargo make check`. The retained-title semantic task is not on the success
path.

Receipts bind working directory, exact tasks/arguments/prefix, Rust 1.97.0,
formatter pin `nightly-2026-07-16`, task-runner hash, former server store 18 identities,
temporary-root choice, noninteractive capture, exit status, complete output bytes
and hashes, and pre/post tree identity. A supplied temporary root is a preselected
short existing absolute real nonsymlinked directory. Do not change it after failure
without classifying setup change.

This phase formats once, performs one workspace compile/static preflight, and
performs one integrated migration/locator preparation pass. It runs no focused
behavioral test staircase, manual per-source patch loop, semantic generator, or
production behavior change. If it finds source defects, preserve all findings,
inspect the whole boundary, make one coherent repair batch, re-freeze the affected
identity, and run only the missing canonical preparation phase on the new exact
candidate.

### Bounded aggregate repair protocol

<a id="rule-PA-VAL-0002"></a>
**[rule:PA-VAL-0002]** The unified protocol runs one initial complete aggregate.
Pass accepts the execution gate. Failure preserves the exact tree and complete
diagnostics, then permits one coherent whole-boundary repair batch without an
individual or filtered test run. Production repair creates a new CORE-FREEZE
identity. Test/wrapper/task/runbook repair creates a new ACC identity. A
cross-boundary repair creates both.

After that batch, run one existing canonical task that covers the complete affected
boundary: `test-rust`, `test`, `test-vnext-server-store-store`,
`test-vnext-retained-title-core`, `test-vnext-architecture`,
`test-vnext-cli-diagnostics`, `check-rust`, `lint`, or `check-node`. Use the
smallest whole boundary; use `test` for cross-test failures. Do not add filters,
individual test names, ad hoc commands, several narrow tasks, or a new task. If no
existing task covers the boundary, stop for validation-architecture review.

A failed boundary task stops the normal repair cycle. A passed boundary permits
one final complete aggregate on the clean rebound candidate. A failed final
aggregate stops. Only the actual complete aggregate can supply full-check evidence.

Setup, fixture, socket, port, permission, database lifecycle, tool selection, or
capture failure does not authorize an unchanged rerun. Preserve evidence, identify
the cause, materially change setup or canonical harness, record it, and allow one
bounded retry. Stop after two materially identical setup failures. D0a keeps its
stricter no-retry rule.

### Package identity inspection

<a id="rule-PA-DEL-0003"></a>
**[rule:PA-DEL-0003]** Reproduce package identity with ordinary read-only tools.
Check the exact owned file set, raw byte counts and SHA-256 values, sorted manifest
rows, manifest exclusion of itself, exact V22 bindings and slices, source-corpus
fingerprints, rule/census references, and clean scope. Do not run a package checker,
build, compiler, formatter, migration, wrapper, test, probe, service, or executable
validation for AR-PKG, AR-CLOSE, or another package-governance amendment.
Mechanical agreement proves identity only. It does not prove historical semantic
fidelity and does not require access to the quarantined private corpus.

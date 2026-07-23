# Decodex vNext Gate Manifest

Status: normative sequencing and acceptance boundary except for the private-artifact subsystem.

Owner: [vNext authority decision](../decisions/vnext-authority.md). Contract:
[vNext authority contract](vnext-authority.md).

## Sequencing rules

XY-1260 establishes authority only. It does not implement PostgreSQL, app-server/Codex
adapters, GPUI, protocol, runtime services, or migration. No later milestone may begin by
reinterpreting a superseded Lane Authority v2 C1-C7 checkpoint. Each gate must record the
exact source revision, command/test evidence, contradictions, and accepted outcome before
its dependent implementation uses the result.

## Downstream ownership

| Range | Accepted downstream ownership |
| --- | --- |
| [XY-1261](https://linear.app/hack-ink/issue/XY-1261)-[XY-1264](https://linear.app/hack-ink/issue/XY-1264), with the failed live gate aggregated by [XY-1304](https://linear.app/hack-ink/issue/XY-1304) | v0.2 freeze and PostgreSQL/blob/cache proof are accepted; the XY-1262 foundation is accepted, XY-1360 owns the still-disabled live-continuation and atomic Context-Pack fallback implementation after V16, XY-1304 owns only its later live-routing aggregate gate and enablement amendment, and XY-1263 accepts only the isolated pinned GPUI foundation. |
| [XY-1265](https://linear.app/hack-ink/issue/XY-1265)-[XY-1269](https://linear.app/hack-ink/issue/XY-1269) | Workspace ownership boundaries, `decodexd` protocol, PostgreSQL persistence, `~/.decodex`/API-only CLI, and the serial P/K/L/S GPUI client decomposition defined below. |
| [XY-1270](https://linear.app/hack-ink/issue/XY-1270)-[XY-1276](https://linear.app/hack-ink/issue/XY-1276), plus [XY-1304](https://linear.app/hack-ink/issue/XY-1304) | Typed app-server, Conversation/RuntimeSession/history, shared-home, vault/runner-binding, quota-calculation, and profile foundations; the XY-1355-XY-1363 reset chain supplies the missing routing authorities and evidence. Private-artifact authority and future delivery are owned only by the [private-artifact authority package](private-artifact/README.md); this row does not restate that sequence. XY-1304 owns only the later live-routing aggregate gate and separate enablement amendment and continues to block the Quick Task slice. XY-1336 is upstream-blocked tracking outside this critical path. |
| [XY-1277](https://linear.app/hack-ink/issue/XY-1277)-[XY-1286](https://linear.app/hack-ink/issue/XY-1286) | Projects/Advisor/Lead, context, messages/collaboration, decision queues, Programs/Objectives, WorkItems, ManagedRuns, repository services, Task-owned independent review/repair/landing, and Project/Program authority policy. |
| [XY-1287](https://linear.app/hack-ink/issue/XY-1287)-[XY-1290](https://linear.app/hack-ink/issue/XY-1290) | Automation definitions/firings, materiality/loop safety, removal of manager agents, and PubFi/SEO/GEO/Radar/Publisher dogfood. |
| [XY-1291](https://linear.app/hack-ink/issue/XY-1291)-[XY-1297](https://linear.app/hack-ink/issue/XY-1297) | GPUI conversations, project/run workspace, graph/timeline, operational surfaces, multi-GB pagination/cache/search, thin menubar, and accessibility/interaction gates. |
| [XY-1298](https://linear.app/hack-ink/issue/XY-1298)-[XY-1303](https://linear.app/hack-ink/issue/XY-1303) | Observability/retention, authenticated remote security/backups, E2E and fault injection, performance budgets, empty-state legacy cutover/removal, and package/dogfood/release reconciliation. |

Each issue is accepted only for its own stated scope and blocked-by relations. The ranges
are navigation, not permission to collapse tasks or skip gates. Linear relations are
planning metadata, not product/runtime identity.

## Required architecture and implementation gates

1. The accepted GPUI exact-revision build/package/test/accessibility foundation
   (XY-1263); production shell acceptance remains the later S gate defined below.
2. The accepted XY-1262 foundation gate: shared-home/process isolation,
   creation-receipt ownership, negotiated app-server contracts, supported
   exact-ID/list/read/archive behavior, lossy-read/divergence policy, native run-local
   collaboration normalization, process-scoped authentication/redaction, read-only
   integrity evidence, and pure duration-typed quota policy.
3. The separate failed XY-1262 live account-routing enablement gate (XY-1304): natural
   quota depletion, durable exclusion before fallback, crash-safe exactly-one
   continuation, real resume-denied Context-Pack fallback, all-depleted wait/wakeup
   readback, side-effect reconciliation, and supported Codex Desktop title discovery.
4. The future private-artifact delivery and acceptance gates in the
   [private-artifact authority package](private-artifact/README.md). This manifest is a
   nonnormative projection for that subsystem and does not restate its exact sequence.
5. Empty PostgreSQL bootstrap, backup/rollback, and concurrent lease/outbox tests
   (XY-1264). The scoped proof choices, measurements, recovery procedure, and downstream
   boundary are recorded in [vNext storage feasibility evidence](../evidence/vnext-storage-feasibility.md).
6. WebSocket reconnect, cursor resume, command idempotency, and current/previous-minor
   compatibility tests (XY-1266 and regression owner XY-1300).
7. Large-history pagination/cache test proving multi-GB history is never eagerly loaded
   (XY-1263, implementation XY-1295, and regression owner XY-1300).
8. ManagedRun restart and side-effect reconciliation fault injection, including the
    Task-owned independent review loop and typed reviewer wait/failure states (XY-1283,
    XY-1285, and regression owner XY-1300).
9. Real Program/Automation/Lead/Task/Reviewer dogfood, using PubFi or equivalent
    (XY-1290 and release dogfood owner XY-1303).
10. Remote binding stays disabled until authentication, TLS, authorization, and
    redaction gates pass (XY-1299).

### XY-1262 foundation acceptance

Manager accepted this split on 2026-07-13. The decision provenance is merged PR #1098 at
`687605583817eca32cbdfb1107f3ee18d3106cea`; that proposal becomes authority only through
this independently reviewed amendment. Repository authority is normative and Linear is
planning metadata.

The [Codex runtime proof](../evidence/vnext-codex-runtime-proof.md), including the merged
reconciliation receipt, accepts the foundation gate for exactly these observed or pure
evidence boundaries:

- one shared normal `~/.codex`, with each app-server process bound to one account and no
  credential switching under a live process;
- Decodex ownership only from a durable creation receipt, never from arbitrary Codex
  history;
- generated typed schema plus negotiated live method results keyed by the Codex build;
- persistent exact-ID, filtered-list, read, explicit archive, and restart readback;
- lossy `thread/read` handling and a fail-closed ManagedRun `diverged` policy;
- native collaboration/subagent events normalized only as run-local actors;
- process-scoped authentication, redaction, and no-mutation integrity evidence; and
- pure quota decisions keyed by duration 300/10080, including unknown, stale, reversed,
  and all-depleted synthetic cases.

Healthy-account same-thread continuation and a manually started Context-Pack session are
mechanism evidence only. They do not accept automatic cross-account routing or fallback.
No global Codex title-search contract is accepted: exact-ID and filtered-list ownership
readback are the supported boundary.

### Permitted foundation work

Permission is issue-scoped and does not bypass each issue's own dependencies:

| Issue | Permitted boundary |
| --- | --- |
| XY-1265 | Workspace ownership cutover and composition roots; no compatibility facade. |
| XY-1266 | Loopback protocol, idempotency, reconnect, backpressure, and non-loopback refusal. |
| XY-1267 | PostgreSQL transactions, leases, outbox, and inert account/window schemas. |
| XY-1268 | Owned `~/.decodex` paths and API-only diagnostics that report unavailable/unknown honestly. |
| XY-1269 | P and K may proceed independently under their own dependencies; L waits for P and K, and S waits for L. P, K, and L remain non-production and default-disabled. Only S owns production shell and exact final-artifact qualification. |
| XY-1270 | Generated typed app-server contracts, live capability negotiation, redaction, and one-account-per-process supervision; no task scheduling or account choice. |
| XY-1271 | Conversation/RuntimeSession/history and inspectable Context-Pack persistence; no automatic rollover, assignment, or fallback dispatch. |
| XY-1272 | PostgreSQL configured-principal and ACL authority manifest/readiness closure against V8; no migration or Codex creation/reconciliation surface. Any future configured role must atomically extend configuration, bootstrap, manifest/readiness, and negative tests. |
| XY-1273 | Credential-vault metadata and immutable runner/account binding; no sticky or policy assignment. |
| XY-1274 | Exact-microsecond quota persistence, `/2` canonical mutation identity, atomic V8 zero-state migration, and durable exclusion transaction tests using synthetic fixtures only; no live exclusion, fallback assignment, or wake scheduling. |
| XY-1275 | Umbrella for user-owned profile persistence and RuntimeSession snapshots. It closes only through the serial XY-1345 -> XY-1346 -> XY-1337 order. Account-owned plugin, skill, and MCP readiness remains typed `unknown`; XY-1336 neither closes nor blocks this issue. |
| XY-1276 | Production Quick Task creation; remains blocked by XY-1304. |
| XY-1300 | Later whole-product E2E, fault, UI, packaging, cutover, and release acceptance after XY-1304; no pre-XY-1304 frozen-core gate ownership. |
| XY-1304 | Live-routing aggregate gate and separate reviewed enablement amendment after XY-1364 frozen-core acceptance. It owns no migration, policy snapshot, candidate construction, experiment schema, continuation, orchestration, wake lifecycle, or Desktop discovery implementation. |
| XY-1345 | Accepted exact-command authority and isolated PostgreSQL 18 prototype only; no production migration or Rust command path. |
| XY-1346 | PostgreSQL V9: separate exact receipts plus immutable global RoleProfile bootstrap/update. Starts only after XY-1345 lands. |
| XY-1337 | Re-bounded RuntimeSession snapshot creation/transition migration, expected V10 after XY-1346. It does not own exact-receipt or RoleProfile redesign. |
| XY-1343 | PostgreSQL V11 canonical WorkItems, transactional readiness blockers, and immutable Lead acceptance; no run execution or completion. |
| XY-1338 | PostgreSQL V12 inert waiting ManagedRuns, exact-run Task/Reviewer assignments, exact RuntimeSession revision binding, FK-backed effect lineage, the fail-closed positive/inconclusive safety transaction, and the forward repair that removes illegal RuntimeSession row locks from V3 Turn/History invoker guards while preserving 1271 serialization. No producer, scheduler, acquisition, dispatch, progress, or completion path. |
| XY-1284 | Accepted two-stage managed-repository authority reset; stage two is finalized by XY-1348 and consumes accepted XY-1354 unchanged. |
| XY-1347 | One bounded macOS/Git feasibility spike for ordinary repositories and linked worktrees; evidence only, with no production source or schema. |
| XY-1348 | Accepted mechanism-neutral transition contract and stage-two PostgreSQL/executor authority boundary; no V13 persistence. |
| XY-1349 | Accepted sole V13 managed-repository PostgreSQL authority migration. |
| XY-1350 | Read-only allocation evidence plus accepted Git/filesystem executor and readback only; may proceed in parallel against the accepted contract, with no persistence, receipt, saga, provider, or shared composition ownership. |
| XY-1351 | First shared repository effect saga path, composing preparation, fresh receipt consumption, execution, readback, and terminal reconciliation; no migration, executor-internal, or provider ownership. |
| XY-1352 | GitHub PR/check effect and reconciliation boundary with explicit provider identities and positive readback; no local repository discovery. |
| XY-1353 | Serial integration, final authority/OpenWiki alignment, deferred-validation inventory, and exact-candidate freeze; it blocks XY-1285. |
| XY-1355 | Normative live-routing authority and capability-applicability reset only; no executable implementation or validation. |
| XY-1356 | Sole V14 migration owner for revisioned complete routing-policy and candidate-set authority. |
| XY-1357 | One post-freeze natural provider timestamp-precision receipt; no deliberate quota consumption or routing enablement. |
| XY-1358 | Sole V15 migration owner for causal positive-only Codex experiment authority. |
| XY-1359 | Sole V16 migration owner plus pure routing kernel and atomic persisted decisions; no dispatch. |
| XY-1360 | Sole V17 migration owner for same-thread continuation and one atomic Context-Pack/RuntimeSession fallback after V16; no dispatch. |
| XY-1361 | Runtime orchestration over persisted authorities with production dispatch structurally disabled. |
| XY-1362 | Sole V18 migration owner for the inert `waiting_usage` wake lifecycle, plus its forward-only V19 deterministic time-authority repair; no selection or production scheduler authority. |
| XY-1363 | Post-freeze retained-title Codex Desktop discovery evidence only. |
| XY-1364 | Frozen-core integration and acceptance for the checked-in V14-V21 ledger, including the forward-only V20 restore canonicalization and V21 RuntimeSession event-reference repair; production routing remains disabled. |
| XY-1367 | Sole V22 owner for the two-effect retained-title experiment bridge and inert manual Rust runner. It does not execute validation or live effects. |
| XY-1368 | Historical mechanical, migration, semantic, authority-digest, and production-isolation acceptance for the exact XY-1367 V22 candidate; it is not current command authority. |

The [XY-1368 retained-title freeze](xy-1368-retained-title-freeze.md) is immutable V22 historical
evidence. It preserves the historical V14-V21 acceptance and records the V22 receipt and deferred
work as they existed at that freeze. It is not current command, task-runner, or V1-V23 delivery
authority. Future retained-title preparation and semantic delivery contracts are owned only by the
[private-artifact package delivery module](private-artifact/operations-delivery.md).

XY-1336 is an upstream-blocked tracking issue outside the M2 critical path. A host file,
manifest, configuration value, remote catalog entry, process binding, or user declaration
cannot close the missing account-owned receipt. Existing doctor `unknown(plugin)` is the
first-release result, and `plugin_unready` remains inert reserved state.

### XY-1356 deferred V14 acceptance inventory

V14 source review precedes executable validation. The frozen integration gate must execute the
following as one coherent boundary, not as per-command repair loops:

- fresh V1-to-V14 initialization and V13-to-V14 forward upgrade; exact migration ledger,
  configured-principal ACL, 60-relation, 119-function, and 110-trigger inventories; derivation and
  binding of the new schema/configured-authority digests; populated dump/restore parity;
- policy replacement with empty, one-member, and multi-member inventories; every omission,
  duplicate, foreign member, order gap, account-revision race, accepted-Policy race,
  RoleProfile race, BuildId mismatch, and same-expected-revision/different-key schedule;
- evidence publication with the exact eight-capability order and every closed state; account,
  process, RoleProfile, BuildId, schema-fingerprint, and evidence-revision conflict schedules;
  explicit rejection of timestamps and causal experiment, plugin, skill, MCP, marketplace,
  OAuth/login-management, account-configuration, host-file, digest-only, and credential-shaped
  sources;
- snapshot resolution under the 1271 -> 1338 -> 1356 coordinator order and fixed SHARE bridge;
  concurrent account, quota, Policy, RoleProfile, RuntimeSession, ManagedRun, evidence, and
  inventory writers; resolution-clock capture only after the full lock/recheck set;
- exact 300-second freshness acceptance and 300-seconds-plus-one-microsecond rejection for
  account/evidence/quota facts; future facts; negative disabled/auth-failed facts independent of
  age; exact raw quota timestamp preservation; separate ordered 300/10080 facts;
- complete policy-order snapshots, one sticky source, exact account/profile snapshot lineage,
  eight-cell capability matrices, explicit applicability, deterministic blocker arrays, and
  deferred commit failure for every partial member, quota, matrix, blocker, or evidence child set;
- byte-identical replay and lost-response recovery for all three operations; same-key changed
  envelope/operation `DX001`; stable rejection replay after later state changes; rollback at every
  receipt/domain/child/response boundary; no committed executing receipt;
- runtime/PUBLIC direct relation, helper, receipt, activity, outbox, trigger, grant, ownership,
  or DDL denial; EXECUTE only for the three public V14 functions; hostile search path, overload,
  function-body, trigger-binding, ACL, default-privilege, and extension-dependency substitutions;
- strict Rust rejection of malformed bytes, unknown/missing/null/defaulted fields, identity
  duplication/reordering, position gaps, quota-pair or capability-matrix drift, blocker/disposition
  inconsistency, cross-linked revisions/provenance, and effect-digest mismatch; and proof that V14
  exposes no account selection, `waiting_usage`, continuation, wake, dispatch, or live enablement.

### XY-1345 exact-command reset gate

XY-1345 owns repository authority and a non-production PostgreSQL 18 proof, not a migration or
Rust product API. Its accepted boundary is operation-specific, command-complete
`SECURITY DEFINER` functions; PostgreSQL-built and -consumed typed request envelopes; a separate
`exact_command_receipts` relation; no runtime receipt-table/private-helper/canonical-audit mutation
authority; deferred commit closure for executing rows; immutable stored response bytes; and
separate stable-domain, idempotency-conflict, and retryable-infrastructure outcomes. It changes no
legacy `command_receipts` semantic.

The isolated proof must pass all deterministic schedules for same-key waiting/replay, changed
envelope and cross-operation conflict, rollback at receipt/domain/activity/outbox boundaries,
precommit connection loss, postcommit lost result, stable rejection replay, deferred incomplete
commit rejection, runtime receipt and canonical-audit denial, `READ COMMITTED`/`REPEATABLE
READ`/`SERIALIZABLE`, opposite-order deadlock and whole-transaction retry, explicit nulls, exact
text variants, typed numeric convergence, fixed scalar bootstrap groups, returned-row effect
binding, catalog closure, and populated dump/restore. It must then pass 50 repetitions each of
32-way identical and mixed-envelope concurrency with zero duplicate domain effects, duplicate
activity/outbox pairs, response mismatches, committed executing rows, authority bypasses,
unexplained rows, or unclassified SQLSTATEs. The durable receipt is
[XY-1345 exact-command evidence](../evidence/xy-1345-exact-command-authority.md).

Any anomaly falsifies the architecture and stops V9. A passing prototype permits only this serial
order:

```text
XY-1345 authority/prototype and fresh exact-candidate review
-> XY-1346 V9 exact receipts plus RoleProfile bootstrap/update
-> XY-1337 V10 RuntimeSession snapshots, creation, and transition
```

V9 must re-prove the complete security-definer catalog/ACL/default-privilege/search-path/dependency
closure, canonical audit namespace closure, clean V1-to-V9 bootstrap, V8-to-V9 upgrade, hostile SQL,
crash/concurrency, and populated restore against the production migration. Candidate 3 SQL and Rust
command code are superseded and cannot be transplanted; its valid invariants and hostile-test ideas
remain provenance only.

V10 must preserve the accepted V9 exact receipt and global RoleProfile contracts while adding only
the two command-complete RuntimeSession entrypoints and their private builders/rejection helper.
Acceptance requires zero-state V9-to-V10 fencing, clean V1-to-V10 bootstrap, exact creation and
transition substitution conflicts, byte-identical success and stable-rejection replay, coherent
old-or-new profile race binding, immutable account/profile history, direct-DML/helper/audit-namespace
hostility, receipt/domain/activity/outbox/response rollback boundaries, whole-transaction retry,
blocked-old-writer cutover, crash/restart convergence, concurrency, and populated dump/restore.
These expensive PostgreSQL 18 gates run once against the frozen serial
XY-1345 -> XY-1346 -> XY-1337 candidate; implementation work does not start a live database.

### XY-1284 managed-repository authority gate

Stage two is accepted. PostgreSQL owns durable projection, generation/tip, global
complete-descriptor operation assignment, append-only evidence, exact compare-and-swap,
atomic command completeness, and restart loads. Pure deciders/facts are mechanism-neutral
and non-authoritative. Complete canonical equality yields
`ExistingExact(OperationView, NoDispatch)`; any difference yields permanent
`OperationIdConflict`. Only a same-control-path successful COMMIT acknowledgement may
mint one fresh affine receipt. Unknown COMMIT outcome, persistence, repeat, readback,
restart, and terminal state provide no receipt and authorize no external execution.

Allocate and its evidence are strictly read-only outside PostgreSQL. `Register`,
`WorktreeReady`, and `Commit` are distinct durably fenced `PossiblyEffected` operations with
operation-specific positive readback and readback-only restart. They permit no retry,
replay, adoption, repair, or import. `Register` requires exact reciprocal registration,
`WorktreeReady` keeps the head unchanged, and `Commit` advances exact `H` to exact `H-prime`
once. Authorized whole-cluster restore may redefine authority inside the trusted
PostgreSQL-administrator boundary; V1 does not automatically detect it. The trusted
single-daemon/same-UID boundary and accepted XY-1354 descriptor-assisted symlink-free
absolute-path reacquisition plus pinned Git 2.54 remain unchanged.

The replacement ownership and dependency graph is:

```text
XY-1284 accepted reset
├── XY-1347 bounded macOS/Git feasibility ─┐
└── XY-1348 pure transition/executor core ─┴─> stage-two authority
                                               ├──> XY-1349 sole V13 persistence ─┐
                                               └──> XY-1350 allocator/executor ───┤
XY-1349 + XY-1350 ────────────────────────────────> XY-1351 effect saga/validation ─┤
XY-1348 + XY-1349 ────────────────────────────────> XY-1352 GitHub reconciliation ──┤
XY-1349 + XY-1350 + XY-1351 + XY-1352 ──────────> XY-1353 integration/freeze
XY-1353 ─────────────────────────────────────────> XY-1285
```

The migration ledger is a singleton serial-writer domain. XY-1349's V13 is accepted on
`main`. The checked-in order continues through XY-1356/V14 durable routing-policy authority,
XY-1358/V15 causal experiment authority, XY-1359/V16 atomic routing decisions, and
XY-1360/V17 continuation authority. XY-1362 owns the V18 inert `waiting_usage` wake lifecycle;
V19 is its forward-only deterministic time-authority repair. XY-1364 frozen-core acceptance
incorporates the forward-only V20 restore canonicalization and V21 RuntimeSession event-reference
repair. XY-1367 adds only forward V22. XY-1368 owns its frozen acceptance. These versions are
allocated in source. A later owner may allocate another migration only
when then-current source inspection proves that additional durable state is required. XY-1350 and
the remaining managed-repository children retain their accepted non-routing ownership.

No managed-repository implementation executes validation before the integration tree is
frozen. One complete unified validation runs once on that exact frozen tree. Its concise
evidence categories are pure semantics; PostgreSQL authority, concurrency, restore, and
retention; accepted Git/filesystem execution and operation-specific readback; the first
shared saga; provider and repository integration; and final digest/manifest agreement.
No partial run, detailed early matrix, or result from another tree is acceptance evidence.

#### XY-1353 deferred acceptance matrix

The Manager runs this matrix once against the exact frozen candidate. Every receipt must bind the
candidate HEAD and tree; no result from an owner branch or earlier integration tree is reusable.

| Boundary | Deferred acceptance cases |
| --- | --- |
| Integration and regression | Exact XY-1349/XY-1350/XY-1351/XY-1352 stack ancestry and exports; one runtime composition; no duplicate migration, executor, saga, provider, receipt, or authority owner; rejected candidate trees remain absent. |
| Pure protocol and schema | Canonical admission/allocation/operation descriptors; global operation-ID conflict and exact-repeat/no-dispatch behavior; Register/WorktreeReady/Commit evidence-kind separation; protocol clients remain isolated from PostgreSQL, filesystem, and provider authority. |
| PostgreSQL authority | Fresh V13 migration and rollback; exact ledger order; runtime ACL/function/trigger/catalog closure; compare-and-swap, immutable evidence, receipt provenance, retention, concurrency, populated dump/restore, and schema/authority digest agreement. |
| Local repository effects | Ordinary and linked repositories; pinned executable/config/environment authority; read-only allocation acquisition; reciprocal registration; unchanged ready head; exact one-head commit advance; dirty, stale, foreign, replaced, symlinked, occupied, rollback, lost-response, and ambiguous readback. |
| Saga and restart faults | COMMIT acknowledgement loss; receipt consumption at every dispatch boundary; dispatch serialization race; crash before/during/after effect and reconciliation; bounded restart enumeration; readback-only recovery; no receipt reconstruction, replay, adoption, repair, or duplicate effect. |
| Restart backlog bound | Zero, below-limit, exact-256, and over-limit eligible restart backlogs; pending work after reconciliation; the one-item residual probe must distinguish exhaustion from residual work and prevent repository readiness without an unbounded loop. |
| Startup failure observability | Missing/replaced pinned Git executable, executor integrity failure, incompatible restart row, and readback/store failure; typed bootstrap readiness, `ServerRepositories` doctor status, and product-state availability must all fail closed while retaining the startup failure classification. |
| Validation supervision | Exact source fingerprint before/after; success, nonzero exit, signal, timeout, cancellation, output limits, drain bounds, descendant teardown, spawn/capture failure, and concurrent protected-worktree mutation. Same-UID hostile-code confinement is not claimed. |
| GitHub effects | Explicit provider/repository/head/base/marker identities; create/update duplicates; lost mutation response; complete multi-page scans; cursor/snapshot/page drift; absent, stale, externally changed, ambiguous, and provider-fault outcomes; required check-run completeness; no CWD/local-Git inference or live mutation without later provider authority. |
| End to end | PostgreSQL admission and allocation through Register, WorktreeReady, Commit, supervised validation, sealed GitHub reconciliation, daemon restart, and authoritative readback on one exact fixture; every failure remains unavailable, pending, or ambiguous rather than successful or replayable. |
| Frozen artifacts | One final migration inventory, configured-authority inventory, class-specific semantic schema manifest, explicit database-binding evidence, separate mutable sequence-state receipt, expected digests, and source/tree binding produced by the canonical unified PostgreSQL gate; collect source, relevant post-command, RoleProfile-restored, and final primary-restored checkpoints, compare every expected/actual digest and row delta in the same invocation, run the existing production restore checks, and preserve restore parity without catalog OIDs or presentation text entering cross-database identity. |

There is no separate checked-in XY-1353 manifest/digest generator. The existing canonical
PostgreSQL harness produces and verifies those inventories only as part of the unified frozen-tree
gate, so source-freeze work must not invent a second generator or execute the harness early.
When the canonical semantic identity or closure model changes, final acceptance uses two exact
phases. Phase A first proves dependency closure plus S0→R1→R2 source/restore parity and emits an
immutable candidate receipt whose mismatch array is the exact canonical subset of schema then
configured authority: zero, either singleton, or both. Phase B runs from the same clean HEAD/tree
when that array is empty; otherwise it runs from one clean direct child that changes exactly the
reported digest array or arrays and nothing else. Phase B verifies both source bindings, the
candidate hash, clean state, mismatch order, and exact transition shape before repeating the full
capture and emitting a fresh acceptance receipt explicitly bound to Phase A. A derivation or older
receipt remains provenance only; malformed, substituted, duplicate, or out-of-binding evidence
cannot attest the Phase B source.

The unified PostgreSQL aggregate is scheduled by one explicit top-level stage graph. Fatal
configuration/cluster preflight covers mode and arguments, clean source binding, private
temporary-root setup, PostgreSQL tool discovery, cluster init/start, and base-role creation. Phase
A/B output and receipt-lineage validation remains direct and outside this graph. Every
meaningful semantic suite has `passed`, `failed`, or `blocked` state. Expected `TestFailure` blocks
only declared consumers and leaves independent branches schedulable; dependency consumers never
run after failure. Required nested restore work is part of its owning suite's outcome: a failed or
unavailable capture, restore, parity check, or production check prevents owner success and blocks
its consumers. A private live-doctor mutation SQL executor alone owns ordinary, role-as, and
secret-bearing mutation process spawn, dispatch/completion facts, output handling, and cleanup; the
coordinator owns doctor readiness and the doctor child. Every mutation and doctor child receives
bounded terminate, kill-fallback, and reap attempts across every exit; an unreaped or indeterminate
child is harness corruption. Mutation probes and restorations are separate stages. Failed ordinary
`Popen` and secret prelude failure are pre-dispatch and block restoration. Successful ordinary
`Popen` owns the SQL payload in argv and makes delivery possible. A secret mutation becomes
may-have-dispatched immediately before its first mutation-frame payload write. Every later write,
flush, timeout, protocol, nonzero-exit, lost-acknowledgement, semantic, or cleanup failure remains
restoration-eligible. Successful exit records command acknowledgement only, not exact server
receipt or a non-vacuous catalog mutation; optional postcondition probes remain separate evidence.
The scheduler consumes exactly one restoration claim from each shared-fixture attempt. An eligible
failed probe still attempts restoration, and failed restoration blocks all subsequent shared-fixture
probes and final evidence.
Unexpected assertion/key/type failures, corrupt stage/report state, source-binding failure,
redaction failure, or another unexpected exception stops new work as harness corruption. After a
private directory is created, its outer cleanup owner covers every later exit and attempts direct
removal if cluster start was never attempted, reporting removal failure without replacing an
earlier primary. After a cluster starts, teardown and final report
emission still run. Aggregate output/report failures are primary only when no earlier failure was
selected; otherwise cleanup and emission failures are recorded as secondary. Only the normal
aggregate emits `decodex/postgres-aggregate-stage-report/1`; focused and Phase A/B modes retain
their direct output and receipt behavior.

Falsifiers are evaluated in this fixed priority order: architecture, then
stability/recoverability, security/authority, verification, integrity, and performance.
An earlier class cannot be traded away for a later-class success.

- **Architecture:** falsified if ordinary and linked repositories cannot preserve one
  explicitly admitted authority without implicit mutable-path rediscovery; if the pure
  contract cannot keep admission, allocation, mutable head, and effect authority
  distinct; or if correctness requires another repository-effect owner beside
  `decodexd`.
- **Stability/recoverability:** falsified if restart or any `allocated`, `registered`,
  `ready`, `PossiblyEffected`, or completion crash boundary cannot be read back without
  retry, replay, adoption, repair, or import; or if unchanged-head worktree
  readiness and exact-once commit head advancement cannot both be represented and
  recovered.
- **Security/authority:** falsified if stale, foreign, symlinked, replaced, dirty, or
  ambiguous state can authorize an effect; if repository-controlled executable behavior
  or path-bearing output cannot be disabled or exactly allowlisted; or if any supported
  operation depends on CWD, ambient config, or implicit repository discovery. Same-UID
  hostile-code confinement is explicitly not a V1 claim.
- **Verification:** falsified if the unified frozen-tree evidence cannot distinguish
  accepted completion from stale, duplicate, rollback, lost-response, or ambiguous
  outcomes.
- **Integrity:** falsified if a durable reservation can be bypassed, identity/revision
  binding can drift, mutation can escape supervised detection, or positive external
  readback cannot reconcile a possibly completed effect without duplication.
- **Performance:** falsified only after the mechanism is otherwise acceptable if bounded
  allocation, Git execution, recovery, or validation cannot meet the later explicit host
  budgets without weakening an earlier guarantee.

Rejected candidate trees `6e20e9b3cf1415cce9b399da173b0410cc4c80dc`,
`6979e3831da772fca3fe0f0e0b4699df642d3a65`, and
`e42212add13af3f702e0ec8966ce3d6a7b682d12` are superseded evidence, not current
authority or compatibility/history migration inputs. Hostile same-UID or multi-tenant
operation remains a separate future UID/sandbox feasibility and authority problem, not a
stage-two residual or V1 promise.

### XY-1263 acceptance and XY-1269 clean-slice reset

XY-1263 landed in PR #1109. Its reviewed candidate was
`de6d028405159a79f1c30a4eeebdae47481e6f25`, with
`NO_BLOCKING_FINDINGS`; merge commit
`d85a808a88af96d50fb4471deb00d13f4301b07d` retains that candidate as its
second parent. The accepted evidence includes the exact-PID normalized cold-launch
Accessibility gate passing 40/40. This proves only the isolated pinned GPUI foundation
and its minimum committed-text accessibility path. It does not authorize a production
shell or close marked-text/IME, production signing/notarization, VoiceOver/Accessibility
Inspector, variable-height history, production graph behavior, or presented-frame gates.

The rejected combined XY-1269 implementation candidate is superseded. Its replacement is
one serial dependency graph:

```text
P: retained WebSocket session contract
K: append-only app-local cache authority
P + K -> L: narrow GPUI client lifecycle and observable connection state
L -> S: narrow GPUI shell plus one exact macOS artifact gate
```

- P owns handshake/session retention, ordered delivery, application confirmation,
  checkpoint identity, idle retention, cancellation, and bounded connect/send/close. It
  owns no filesystem, retry policy, GPUI, or signing.
- K is a private, app-local, GPUI-independent module within an existing client/application
  owner. It is not a new crate and must not reuse the server-side
  `decodex_core::BoundedCache`. K owns append-only immutable-generation publication and
  invalidation, physical bounds, preservation of uncertain objects, and offline disposal
  of a whole generation.
- L composes P and K. It owns retry, cancellation, quarantine lifetime, minimal state
  application, and one narrow shell-facing connection view. It is not a general
  projection framework.
- S owns window, navigation, focus, and rendering. Its exact final candidate runs one
  package/signing/Accessibility qualification. Packaging is an S acceptance gate, not a
  fifth implementation child.

At the current-main snapshot, P belongs to the existing
`crates/decodex-protocol/src/` client-contract owner and K, L, and S belong to the
existing `apps/decodex-gpui/` application owner. `crates/decodex-protocol/src/lib.rs` is
P's serial export integration surface. P's retained session contract must not inherit the
filesystem/config responsibility of sibling client-profile code in that owner.
P first owns only the stale-diagnostic alignment in `apps/decodex-gpui/src/main.rs`; that
file is otherwise shared in the L-then-S sequence. K remains private beneath the
application owner. No child may create a GPUI cache crate, put K in `decodex-core`, move
client state into the daemon, or treat the isolated `spikes/gpui/` harness as production
source. Each child must freeze its exact existing and added files from its then-current
clean `main` before dispatch; this map names owners, not speculative future filenames or
permission for an unrelated manifest, lockfile, packaging, or native-receipt edit.

Current `main` remains a disabled print-and-exit GPUI composition root throughout P, K,
and L. No lower-level landing enables production UI. P and K are non-rendering and may
proceed under their own dependencies, L starts only from accepted P and K, and only S may
replace the disabled posture after its exact production artifact gate passes.
The checked-in `apps/decodex-gpui/src/main.rs` diagnostic still says XY-1263 remains
failed. That wording is stale, not runtime or gate authority; P owns aligning the
diagnostic with the accepted foundation and disabled P/K/L/S posture before P validation.

The dirty `xv/xy-1269-gpui-shell` branch, its combined candidate, and its evidence are
prototype provenance only. Do not inspect them as executable authority, rebase them into
a product candidate, or salvage mixed manifests, lockfiles, or native receipts. A clean
current-main child may use only independently re-derived small pure designs, tests,
protocol shapes, threat models, or navigation constants.

### XY-1315 candidate-4 identity-ingress authority

Candidate 3 remains frozen. Candidate 4 may start only after this amendment lands and the
existing owner moves to the new base. The Project/Agent slice remains atomic and is
limited to exactly the previously frozen candidate-3 twelve-path envelope below:

```text
crates/decodex-core/src/lib.rs
crates/decodex-postgres/src/authority.rs
crates/decodex-postgres/src/lib.rs
crates/decodex-postgres/src/migrations.rs
crates/decodex-postgres/src/types.rs
crates/decodex-postgres/tests/postgres_store.rs
scripts/vnext/postgres_store_test.py
tests/scripts/test_vnext_architecture.py
crates/decodex-core/src/agent.rs
crates/decodex-core/src/project.rs
crates/decodex-postgres/migrations/V5__project_agent_authority.sql
crates/decodex-postgres/src/project_agents.rs
```

Candidate 4 may modify only this set. Any path-envelope change stops for scope review.
The candidate requires bounded delta tests, the retained aggregate PostgreSQL harness,
canonical validation, and a fresh exact-candidate review.

The candidate-4 boundary is:

1. One schema-qualified ingress-only checked text domain,
   `decodex.canonical_uuid_v4_text`, owns exact lowercase hyphenated UUID-v4 boundary
   spelling. It is not durable identity storage.
2. The closed `pg_proc` inventory contains only these three externally executable
   identity-bearing mutator signatures:

   ```text
   decodex.bootstrap_advisor(decodex.canonical_uuid_v4_text)
   decodex.create_project(decodex.canonical_uuid_v4_text,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.jsonb,decodex.canonical_uuid_v4_text)
   decodex.transition_project(decodex.canonical_uuid_v4_text,pg_catalog.int8,decodex.project_status)
   ```

   No `pg_catalog.text`, `pg_catalog.uuid`, polymorphic, or other identity-mutator
   overload exists. Exact-domain, unknown-literal, explicit-`pg_catalog.text`, and
   prepared/bound-`pg_catalog.text` expressions may resolve to these signatures. Every
   non-null form must pass `canonical_uuid_v4_text_exact` before function execution.
3. The domain is not universal NULL authority. Every mutator remains `CALLED ON NULL
   INPUT` and rejects every null identity parameter in its first executable statement,
   before casts, locks, reads, or writes. Every such rejection uses exactly SQLSTATE
   `23514`, constraint name `canonical_uuid_v4_text_ingress`, and the fixed input-free
   message `identity ingress requires canonical UUID-v4 text`. These literals are shared
   by every identity mutator; parameter names and values never enter the error.
4. Validated domain text converts locally to UUID. UUID table columns retain durable
   semantic version/variant checks, PK/FK, uniqueness, lifecycle, revision, and restore
   authority.
5. Rust continues to bind the original typed ID string as wire text and spells every
   identity argument `$n::pg_catalog.text::decodex.canonical_uuid_v4_text`; it never
   binds a domain OID.
6. Explicit caller normalization through
   `uuid::text::decodex.canonical_uuid_v4_text` remains accepted. Lexical authority
   begins after that explicit normalization, at the resulting domain text value;
   discarded pre-boundary spelling is outside the contract. Under the attested catalog,
   a bare `pg_catalog.uuid` expression must not resolve to a domain-bearing mutator and
   returns SQLSTATE `42883`.
7. Runtime retains SELECT-only Project/Agent table access and no direct DML.
8. Readiness and every live revalidation reject any direct implicit
   `pg_catalog.uuid` -> `pg_catalog.text` cast. That cast audit is not standalone
   authority: the same pass retains the closed `pg_proc` inventory, schema ownership and
   CREATE restrictions, extension/dependency checks, qualified calls and fixed
   function-local search paths, domain/type ACLs, PUBLIC/default-privilege closure, the
   exact runtime EXECUTE allowlist below, SELECT-only Project/Agent table access, and no
   runtime Project/Agent DML.

   At `ba09238b189da12ad60c2a6a3e10c0c60d1c5c52`, current-state audit evidence shows
   the legacy runtime identity can execute 33 canonical signatures: the 15 required
   non-trigger routines below plus 18 trigger-only routines. The nineteenth trigger-only
   routine, `decodex.capture_history_item_version()`, is already non-executable. That
   effective 33-signature set is evidence of legacy overgrant, not the target allowlist.

   Candidate 4's target runtime EXECUTE allowlist contains exactly these 15 existing
   non-trigger signatures required by current persistence behavior:

   ```text
   decodex.is_canonical_media_type(pg_catalog.text)
   decodex.is_history_metadata_projection(pg_catalog.jsonb)
   decodex.normalize_unicode_whitespace(pg_catalog.text)
   decodex.ascii_lower(pg_catalog.text)
   decodex.has_credential_material(pg_catalog.text)
   decodex.has_credential_material(pg_catalog.jsonb)
   decodex.is_meaningful_evidence(pg_catalog.jsonb)
   decodex.rfc3339_utc(pg_catalog.timestamptz)
   decodex.is_valid_operation_duration(pg_catalog.interval)
   decodex.lease_ttl_milliseconds(pg_catalog.interval)
   decodex.try_acquire_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.interval)
   decodex.renew_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.interval)
   decodex.release_lease(pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid)
   decodex.prune_history_snapshots()
   decodex.issue_history_cursor(pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int4)
   ```

   Candidate 4 adds its exact identity-mutator signatures as the additional nested
   domain-only subset. It revokes runtime and PUBLIC EXECUTE from all 19 trigger-only
   routines:

   ```text
   decodex.enforce_lease_operation_time()
   decodex.enforce_outbox_operation_time()
   decodex.forbid_mutation_of_activity()
   decodex.enforce_outbox_terminal_retention()
   decodex.forbid_outbox_truncate()
   decodex.enforce_command_receipt_state()
   decodex.acquire_hierarchy_coordinator()
   decodex.canonicalize_created_at()
   decodex.enforce_blob_object_state()
   decodex.enforce_conversation_state()
   decodex.enforce_runtime_session_state()
   decodex.enforce_turn_state()
   decodex.enforce_history_item_state()
   decodex.capture_history_item_version()
   decodex.enforce_artifact_state()
   decodex.enforce_artifact_revision_state()
   decodex.enforce_context_pack_state()
   decodex.enforce_context_pack_source_state()
   decodex.enforce_history_cursor_state()
   ```

   Broad `GRANT EXECUTE ON ALL FUNCTIONS` is prohibited. Rebase-time audit must preserve
   the distinction between observed legacy grants and the required target authority.
9. The migration revokes PUBLIC privileges on every existing protected routine and
   type/domain. The migration owner also establishes global owner default-privilege
   revocations for PUBLIC EXECUTE on future routines and PUBLIC USAGE on future types,
   then applies only the exact required grants.
10. The canonical schema manifest/digest, tamper checks, and populated logical restore
    attest the domain base type, collation dependency, validated constraints,
    `typnotnull`, owner, type ACL, function signatures/source/settings/ACL/dependencies,
    relevant owner/default ACLs, exact runtime grants, and the exact NULL-rejection
    constraint-name and message literals.
11. SECURITY DEFINER relations and types are schema-qualified, the owner is unreachable,
    and function settings are attested under a hostile search path. Null guards and
    conversion precede advisory locks; Project writes precede Lead writes; propagated
    exceptions preserve atomic rollback.
12. Candidate 4 preserves the resolved credential/privacy, split-identity, concurrency,
    dot/C0/DEL/C1 path, lifecycle, restart, collation, tamper, digest, rollback, and
    populated-restore obligations.

Candidate 4's ingress suite uses separately named cases for exact-domain,
unknown-literal, explicit-text, prepared/bound-text, invalid-text, null, bare-UUID, and
explicit-`uuid::text::decodex.canonical_uuid_v4_text` expressions. Every case carries
its exact SQL expression in the assertion/report so text and UUID outcomes cannot be
grouped or falsely attributed.

These requirements are one authority boundary. A domain check does not replace durable
UUID constraints, explicit NULL guards, function- and default-privilege closure, or
logical-restore attestation.

### Post-V4 authority order and writer map

This amendment is based on landed `main` at
`2f5e637a2c65ee88c1946df22d5c3649f664f467`. That tree contains the XY-1273
`V4__account_readiness.sql` migration and no later migration. An unlanded migration
name or number is not a reservation. Rebase each schema owner onto the then-current
ledger and allocate its migration version only in the exact candidate that is ready to
land.

Repository decisions, specifications, migrations, source contracts, and tests are the
normative authority. Linear issue descriptions and relations are executable planning
metadata and must be kept aligned with that authority; they cannot amend it. The
post-V4 serial schema and semantic order is:

1. landed XY-1273/V4 account readiness and immutable runner binding;
2. XY-1315 inert canonical Project and Agent identity, including one global Advisor
   identity and one canonical Lead identity per active Project, without live
   Conversation or Codex behavior;
3. XY-1316 minimal Project-owned, versioned Policy identity and immutable accepted
   revisions, without effective policy application;
4. repaired XY-1281 Program and finite Objective persistence, importing the canonical
   Project, Agent, and exact Policy revision authorities; and
5. XY-1282 WorkItem identity and persistence plus its normalized, project-scoped
   Objective-WorkItem association.

Project, Agent, and Policy identity therefore precede Program persistence. WorkItem,
not Program or Objective, owns the normalized association. Both sides must be
foreign-key backed, must resolve to the same Project, and must reject cross-Project
links. A `uuid[]`, JSON array, unconstrained UUID, placeholder identity, or equivalent
denormalized shortcut is not authorized.

XY-1281 owns the forward-only V7 schema after canonical V5 Project/Agent and V6 Policy.
V7 contains no WorkItem identity or denormalized WorkItem relation. Program mutations
verify the same active Project and canonical active Lead plus the exact accepted Policy
revision. Objective achievement is not a bare lifecycle update: one immutable, exact
prior-revision acceptance-and-validation record and the achieved revision are committed
coherently. This evidence is Objective outcome authority only. Every new command reserves,
mutates or records a typed deterministic rejection, appends activity/outbox when changed,
and completes its exact response receipt in one transaction, so rollback cannot expose a
pending receipt and exact retry/reopen returns the same typed result.
ManagedRun may reach successful terminal completion only from explicit authoritative
WorkItem acceptance and validation. Objective achievement or evidence and any external
Codex Goal state cannot establish WorkItem acceptance or ManagedRun success; XY-1282 and
later managed-execution owners must implement that positive authority.
Mutation receipt scope comes only from the canonical Project ID in the request; a missing
aggregate never substitutes an invented scope, and PostgreSQL independently compares that
request scope with stored authority before mutation. Concurrent commands for one absent
Program, Objective, or Objective evidence identity converge without leaking a uniqueness
error: every command completes and replays while only the inserting command emits activity.
Achievement chronology is anchored to the exact prior Objective revision `updated_at`, and
all persisted Program/Objective timestamps share the closed `ProgramTimestamp` range.

The executable dependency edges that mirror this order are XY-1273 -> XY-1314;
XY-1314 -> XY-1315, XY-1317, and XY-1318; XY-1315 -> XY-1316; XY-1315 and XY-1316 ->
XY-1281; the additional direct edge XY-1273 -> XY-1281; and XY-1315, XY-1316, and
XY-1281 -> XY-1282. The direct XY-1273 block is recorded independently rather than
treated as transitively satisfied through XY-1314. The repository order governs if
planning metadata drifts. No fixed Domain Agent hierarchy is part of this order:
additional agents remain a later policy/workload decision.

#### Conflict domains and integration order

PostgreSQL migrations, the embedded migration inventory, schema/authority digest,
aggregate PostgreSQL test harness, and migration evidence are one non-commutative
writer domain. Exactly one schema owner may be active: XY-1315, then XY-1316, then the
repaired XY-1281 persistence slice, then the XY-1282 WorkItem/relation persistence
slice. Pure application work may run beside that lane only when it does not edit or
claim those surfaces.

XY-1317's intended conflict domain is the Codex exact-ID/list/read/archive contract.
On this landed tree, however, typed thread identity/projections are in
`crates/decodex-codex/src/protocol.rs`, while request execution and the only scripted
fake app-server fixture are in
`crates/decodex-runtime/src/account_launch/process.rs`,
`crates/decodex-runtime/src/account_launch/protocol.rs`, and
`crates/decodex-runtime/tests/fixtures/fake_app_server.py`. Its current
"Codex-adapter-only" brief does not authorize that runtime-owned production and fixture
surface. XY-1317 is therefore not dispatch-ready until its planning metadata is
rebriefed to name the runtime transport/fixture ownership or the acceptance contract is
split so the adapter-only candidate is independently testable. Do not duplicate the
fake server under `decodex-codex` to evade this boundary.

XY-1318 owns only pure, side-effect-free quota value and decision contracts in
`decodex-core`. It does not own PostgreSQL quota rows, exclusion receipts, app-server
rate-limit decoding, account assignment, scheduling, wakeup, continuation, or any
live-routing entry point. The existing
`openwiki/evidence/fixtures/xy-1262-quota-matrix.json` is read-only accepted input for
its tests, not a child-owned evidence file.

### XY-1274 exact persistence and zero-state migration gate

XY-1274 is the serial PostgreSQL storage, migration, receipt, activity, outbox, restore, and
application-adapter owner for quota observations and inert exclusions. Its next implementation is
candidate 1 of the materially redesigned exact-microsecond/zero-state boundary. It is not a fourth
repair of the three rejected rounding/populated-conversion candidates preserved in
[the runtime proof](../evidence/vnext-codex-runtime-proof.md).

The implementation gate must prove all of the following on one exact candidate:

- storage accepts only `QuotaTimestampMicros(i64)` in
  `0..=253402300799999999`; raw RFC3339 offsets normalize to equivalent UTC integers, while
  sub-microsecond, pre-Unix, post-year-9999, infinity, overflow/carry, leap-second, and unsupported
  values fail before receipt reservation, with no rounding or truncation path;
- checked integer-microsecond freshness accepts exactly 300 seconds and rejects 300 seconds plus
  one microsecond, including overflow boundaries;
- `decodex/quota-window-mutation/2` and `decodex/quota-exclusion-mutation/2` golden vectors use
  typed logical values, integer timestamps, recursively sorted object keys, preserved array/scalar
  distinctions, one serialized byte sequence, SHA-256 plus byte length, and exact completed-response
  replay without requiring full request-document retention;
- V8 takes `ACCESS EXCLUSIVE` locks on `command_receipts`, `quota_windows`, `activity`, and
  `outbox` in canonical writer order before one transaction structurally rejects every pre-V8 quota
  row; every `mutate_quota_window`/`quota_windows` receipt regardless of state; activity classified
  by aggregate kind, event kind, or structured payload; outbox classified by aggregate fields or a
  structured activity envelope; every outbox link to classified activity; and every malformed or
  orphaned combination;
- no correlation-key or aggregate-ID pattern substitutes for structural classification, and no
  concurrent V7 writer can enter between the assertion and DDL;
- the proven-empty `quota_windows` table is altered in place, preserving table identity, ACLs,
  account foreign key, unchanged observation-index identity, and migration atomicity; the exact
  PostgreSQL 18 authority/schema manifest and digest are regenerated by the later implementation;
- empty V7 migrates, any classified pre-V8 state fails atomically with a stable incompatibility
  result, and whole disposable-database recreation succeeds; no conversion, quarantine, hand
  deletion, retention bypass, table drop/recreation, dual schema, or fallback is accepted; and
- monotonicity, idempotency, exact replay, crash, retry, concurrency, dump/restore, credential
  rejection, separate 300/10080 facts, and inertness pass without creating assignment, fallback,
  scheduling, wakeup, continuation, external-effect replay, or live-dispatch consumers.

XY-1302 alone owns the final whole-ledger squash/reset, production baseline, privilege
provisioning, database-recreation runbook, cutover/rollback readback, and proof that no pre-release
database becomes production state. Those operations are not XY-1274 acceptance work.

The landed-tree path-and-contract ownership map for the post-amendment wave is below.
Every concrete path named in it exists at the pinned snapshot. The map intentionally
does not choose filenames for future Project, Agent, quota, PostgreSQL, or evidence
modules. Each active child must re-derive and freeze its exact additions from the then-
landed tree before dispatch; a proposed new filename has no authority merely because it
fits the directory and contract owner recorded here.

| Surface | XY-1315 Project/Agent identity | XY-1317 exact-ID adapter | XY-1318 pure quota algebra | Serial integration or exclusion |
| --- | --- | --- | --- | --- |
| Production domain source | Sole contract writer for canonical Project/Agent identity, lifecycle, validation, and repository ports added within the existing `crates/decodex-core/src/` owner directory; no account, quota, or Conversation contract. Exact additions require the child pre-dispatch re-derivation above. | Intended existing owner is `crates/decodex-codex/src/protocol.rs`, but no production path is active while the runtime execution gap above holds dispatch. | Sole contract writer for pure duration-typed quota values and policy algebra added within the existing `crates/decodex-core/src/` owner directory; no Project/Agent types or persistence. Exact additions require the child pre-dispatch re-derivation above. | `crates/decodex-core/src/lib.rs` is a shared crate export root: XY-1315 integrates its exports and lands first; only after that landing may rebased XY-1318 become the serial writer for its quota export. Neither child is finally acceptable before its own exact export is integrated and reviewed. |
| PostgreSQL production source | Sole wave writer for Project/Agent persistence within the existing `crates/decodex-postgres/src/` owner directory, including required integration in the existing `lib.rs`, `types.rs`, `authority.rs`, and `migrations.rs`. Exact additions require the child pre-dispatch re-derivation above. | Excluded. | Excluded. | No other active task may edit any `crates/decodex-postgres/src/` file during the XY-1315 schema slice. |
| Migration and schema contract | Sole owner of exactly one new migration under `crates/decodex-postgres/migrations/`, versioned only after rebase onto the landed ledger. | Excluded. | Excluded. | The directory, embedded ledger, runtime grants, schema/authority inventory, clean-install/restore contract, and migration number are one serial domain. |
| Focused tests | Project/Agent core tests stay with the child-selected owning source under `crates/decodex-core/src/`; Project/Agent store cases belong in the existing `crates/decodex-postgres/tests/postgres_store.rs`. | Adapter-owned typed-result tests would belong beside its production owner; live request/response tests currently require the held runtime surface. | Pure table, boundary, fake-clock, overflow, ordering, and property-style tests stay with the child-selected owning source under `crates/decodex-core/src/`. | `crates/decodex-postgres/tests/postgres_store.rs` belongs exclusively to XY-1315 during this wave. Shared architecture acceptance remains serial. |
| Fixtures | Project/Agent SQL/harness data only inside the PostgreSQL aggregate test or its existing harness. | `crates/decodex-runtime/tests/fixtures/fake_app_server.py` is required but not authorized by the present brief; no active writer until rebrief. | Reads, but must not edit, `openwiki/evidence/fixtures/xy-1262-quota-matrix.json`. | No child may repurpose another child's fixture or copy an existing fixture to create a nominally disjoint path. |
| Migration harness | Sole writer for `scripts/vnext/postgres_store_test.py` and any Project/Agent assertions required by the existing PostgreSQL 18 harness. | Excluded. | Excluded. | Harness edits land with the schema candidate, never from a parallel pure-core or adapter branch. |
| Evidence | Sole contract owner for any Project/Agent-specific receipt under the existing `openwiki/evidence/` directory, containing only commands actually run against its exact candidate; no new evidence filename is authorized until the child map is re-derived. Existing XY-1273 evidence remains immutable provenance. | No evidence path is active while dispatch is held. | Test output is validation; it must not rewrite XY-1262 evidence or claim natural-depletion/live acceptance. | Existing files under `openwiki/evidence/`, including `xy-1273-account-runner-binding.md` and `vnext-codex-runtime-proof.md`, are not wave integration scratch space. |
| Contract surface | Canonical Project/Agent IDs, role uniqueness, lifecycle/revision, Project repository/root/default-cwd facts, and inert repository operations. | Exact-ID/list/lossy-read/archive typed facts only; no mapping persistence, divergence, protocol DTO, or live continuation. | Duration-typed 300/10080 observations, fail-closed pure eligibility/exclusion facts, and a hypothetical side-effect-free earliest-ready value. | Cross-domain application composition, runtime WebSocket, protocol, GPUI, and live behavior are excluded from this wave. |
| Crate export roots | `crates/decodex-postgres/src/lib.rs` belongs solely to the active schema writer. | `crates/decodex-codex/src/lib.rs` and `crates/decodex-runtime/src/lib.rs` have no active writer while XY-1317 is held. | Becomes serial writer for its `decodex-core` export only after XY-1315 lands and this child rebases. | `crates/decodex-core/src/lib.rs` follows the XY-1315-then-XY-1318 landing sequence; `crates/decodex-protocol/src/lib.rs` is excluded from the wave. Any required runtime module-root edit must be named by a repaired XY-1317 brief before dispatch. |
| Architecture registry | Serial writer for its Project/Agent guards in the first landing candidate. | No active writer while dispatch is held. | Becomes serial writer for its quota guards only after XY-1315 lands and this child rebases. | `tests/scripts/test_vnext_architecture.py` is a shared registry. Each landing candidate adds only its exact-owner guards, reruns the registry and full check, and receives fresh review before landing. |
| Crate manifests | No dependency edit without separate authority. | No dependency edit without separate authority. | No dependency edit without separate authority. | `crates/decodex-core/Cargo.toml`, `crates/decodex-codex/Cargo.toml`, `crates/decodex-postgres/Cargo.toml`, and `crates/decodex-runtime/Cargo.toml` are excluded unless a separately reviewed serial change authorizes the exact dependency/feature contract. |
| Root build graph | Excluded. | Excluded. | Excluded. | `Cargo.toml`, `Cargo.lock`, and `Makefile.toml` are serial shared integration surfaces. Opportunistic dependency, workspace, task, root-manifest, or lockfile edits are prohibited in the parallel wave. |
| Normative OpenWiki | No child owns final authority edits. | No child owns final authority edits. | No child owns final authority edits. | `openwiki/decisions/vnext-authority.md`, `openwiki/specs/vnext-authority.md`, and this manifest are shared normative authority. A separately authorized serial authority step owns any required semantic amendment; ordinary evidence must not rewrite them. |

Parallel implementation and landing are different gates. The landing order for the safe
subset is XY-1315 first, then XY-1318. XY-1315 integrates its Project/Agent exports and
architecture guards, passes fresh review, and lands as the sole schema writer. XY-1318
then rebases onto that exact landing, integrates its quota export and architecture guard
as the now-serial owner, reruns its focused tests plus canonical validation, receives a
fresh exact-candidate review, and lands second. An excluded or deferred shared file may
be absent during parallel implementation, but the affected child cannot claim final
acceptance or landing readiness until its ordered serial integration is complete. Any
upstream landing that changes a mapped path, crate boundary, schema ledger, fixture
owner, contract, manifest, or test registry invalidates this table for the affected
child and requires the map to be re-derived from the new landed tree before work
resumes.

At this snapshot, the maximum safe concurrent implementation subset is **XY-1315 and
XY-1318**. Their owned production paths and contracts are disjoint once the core export
root and architecture registry are deferred to serial integration. XY-1317 is excluded
from dispatch until its adapter/runtime ownership contradiction is repaired. This is not
permission to land XY-1315 and XY-1318 concurrently or to call either complete before
the shared integration and fresh exact-candidate review.

XY-1304 remains the sole owner of live account-routing enablement. Nothing in this
ordering or writer map enables sticky or policy assignment, quota-driven fallback
assignment, `waiting_usage` scheduling/wakeup, automatic cross-account resume,
automatic Context-Pack fallback, or replay after ambiguous side effects. Every one of
those paths remains hard default-disabled, and unknown or stale quota remains
ineligible, until XY-1304 passes through an independently reviewed repository authority
amendment.

All XY-1270-XY-1275 capabilities must be mechanically inert or default-disabled at their
live boundary. Synthetic fixtures can validate representation, calculation, and
transaction ordering but cannot satisfy the live gate. Readiness cannot authorize
eligibility, assignment, reassignment, fallback, scheduling, wakeup, continuation, or
production routing.

<a id="xy-1372-private-artifact-capability-and-consumption-gate"></a>

### Private-artifact delivery projection

This section is nonnormative. The accepted
[private-artifact authority package](private-artifact/README.md) is the sole normative authority
for private-artifact semantics, ownership, sequencing, and acceptance. The
[XY-1372 capability evidence](../evidence/xy-1372-private-artifact-capabilities.md) remains an
evidence input only. Earlier XY-1373 and XY-1371 wording on this page does not compete with or
amend the package.

The package [delivery module](private-artifact/operations-delivery.md) defines future
CORE-FREEZE and ACC contracts, future canonical preparation and retained-title task contracts,
one future mechanical preparation pass, and the later unified validation protocol. Those are
delivery targets, not implemented behavior. Current source inspection shows no package-defined
private-artifact runtime/API composition, and current task-runner names do not yet implement the
package-defined command surfaces. Use the package for the exact order, owners, path sets,
inventories, and stop conditions; do not copy them into this manifest. No private-artifact gate by
itself enables production routing.

AR-CLOSE accepts signed C2 as the cutover baseline by explicit policy and retires historical
private-corpus semantic rereview as a prerequisite. Its accepted authority identity gates A0 and
D0a. After that acceptance, both can begin and the package-owned downstream
A1/B/C/D/CORE-FREEZE graph remains unchanged. The historical corpus has no runtime or normal
workflow edge, and no private-review channel component exists.

### Failed live account-routing enablement gate

The [live gate issue](https://linear.app/hack-ink/issue/XY-1304) remains failed and
fail-closed. It follows XY-1364 frozen-core acceptance and owns only the live-routing aggregate
evidence gate plus a later separate repository-authority enablement amendment. Production routing
remains structurally default-disabled until XY-1355, V14-V21, disabled orchestration, scheduler
wake, natural timestamp evidence, and Desktop discovery have all landed and the XY-1304 aggregate
gate passes.

The required dependency order is:

```text
XY-1355 authority amendment
-> XY-1356 / V14 complete routing-policy authority
-> XY-1358 / V15 causal experiment authority
-> XY-1367 / V22 retained-title experiment bridge
-> XY-1359 / V16 atomic routing decisions
-> XY-1360 continuation and atomic Context-Pack fallback
-> XY-1361 runtime orchestration with dispatch disabled

XY-1359 -> XY-1362 scheduler-owned waiting_usage wake lifecycle
XY-1355 -> XY-1357 natural timestamp precision evidence

V14-V21 implementation children -> XY-1364 accepted frozen-core base
XY-1364 + XY-1367 -> XY-1368 frozen mechanical and semantic acceptance

XY-1368 + XY-1357 + XY-1363 -> XY-1304 live-routing aggregate gate
-> separate reviewed repository amendment to enable production routing
-> XY-1300 later whole-product E2E/fault/UI/packaging/cutover acceptance
```

The private-artifact prerequisites and delivery path into XY-1363 are not restated in this
diagram. The sole normative sequence is in the
[private-artifact package delivery module](private-artifact/operations-delivery.md). `XY-1368` in
the remaining routing edge denotes accepted V22 historical evidence; it is not current command
authority.

The aggregate gate must bind one exact source tree and prove PostgreSQL-produced complete
routing snapshots and decisions. Caller omission, reordering, substitution, duplicate facts,
or stale revisions must not change the authoritative universe. Every inventory member has an
explicit disposition; unknown or omitted members block. Sticky affinity is bound to its exact
RuntimeSession revision and wins only when independently eligible. Required capabilities and
their applicability are explicit: unknown never satisfies a required capability, while an empty
required-capability set makes unknown plugin inventory non-applicable rather than positive
readiness evidence.

The natural provider receipt retains the exact raw timestamp and must convert exactly to UTC Unix
microseconds without rounding or truncation. A precision-incompatible receipt leaves routing
disabled and reopens only ingress authority. A naturally depleted account, never deliberately
exhausted for evidence, must return the typed quota failure for a fixed no-tool marker. Durable
readback must bind the submitted turn, unknown side-effect state, exact duration-typed exclusion,
complete decision evidence, and either exactly one supported same-thread continuation or exactly
one atomic Context-Pack RuntimeSession after genuine denied/incompatible evidence. All-depleted
state must persist the exact earliest-ready time and one restart-safe scheduler wake that performs
fresh resolution.

Crash injection at every external-effect boundary must produce no duplicate turn, tool,
repository, worktree, Git, or artifact effect. Possibly side-effecting turn replay remains under
the accepted ManagedRun submitted-turn/effect-barrier and repository-effect reconciliation
authorities, not routing. Host-owned before/after receipts prove no-mutation integrity only. The
experiment and gate must not use plugin, skill, MCP, marketplace, login-management, OAuth-
management, or account-configuration inventory/mutation calls to manufacture readiness. XY-1363
must independently prove supported retained-title Desktop discovery after normal indexing without
deriving absence from pagination, list exhaustion, missing events, or lossy readback.

Until all of that evidence passes and a later explicit repository amendment enables the
path, sticky or policy assignment, quota-driven exclusion causing another assignment,
`waiting_usage` scheduling/wakeup, automatic cross-account same-thread resume, automatic
Context-Pack fallback, and replay after an ambiguous or possibly side-effecting outcome
are hard default-disabled. Unknown, missing-duration, stale, or low-confidence quota is
not eligibility evidence and remains fail-closed.

XY-1276 remains blocked by XY-1304. The same direct live-gate block is required for later
issues whose stated acceptance would exercise live routing: XY-1277-XY-1280, XY-1283,
XY-1285, XY-1287, XY-1289-XY-1292, XY-1300, XY-1302, and XY-1303. Their presence in a
later milestone, a synthetic test, or an otherwise completed dependency cannot authorize
managed production routing. Other later foundation or UI work may proceed only when its
own scope can remain inert and its other gates pass; it cannot claim live-routing,
dogfood, cutover, or release acceptance.

The dirty combined XY-1304/V14 candidate, partial fourth repair, caller-authoritative request
shape, Rust authorization wrapper as provenance, global `SupportedPositive` plugin requirement,
combined experiment/routing schema, and sequential exclusion -> RuntimeSession -> decision
composition are superseded and must not be revived. Before core freeze, executable validation is
deferred: no formatter, compile/check/lint, migration or SQL parser, tests or matrices, wrappers or
generators, PostgreSQL, live experiments, or UI/Accessibility/Desktop checks. After freeze,
XY-1364 owns one mechanical preflight, one unified complete core gate, coherent batched repairs,
and one final aggregate rerun. XY-1304 follows with live-routing evidence and its reviewed
enablement amendment. XY-1300 then owns the later whole-product E2E, fault, UI, packaging,
cutover, and release acceptance.

That paragraph records the historical routing-core freeze sequence. Its lower-case "core freeze"
and unified core gate are not the future private-artifact CORE-FREEZE or unified validation
contracts.

#### XY-1358 deferred acceptance matrix

This source-only matrix is deferred to the XY-1364 unified frozen-core gate. It must run against
one exact tree and may not enable a live experiment while collecting evidence.

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Preparation and crash | Crash before preparation leaves no authority; crash after preparation leaves only `prepared`; a ManagedRun or RuntimeSession advance after preparation makes the pre-effect fence reject without revision 2; crash after the committed pre-effect fence leaves terminal `creation_possible` and cannot authorize another create. |
| Lost response and replay | A discarded successful command response replays byte-identically, while replay of the pre-effect fence is typed only as ambiguity and cannot recreate its private fresh permission; a lost app-server creation response never retries creation, searches broadly, adopts a thread, or converts ambiguity to absence. |
| Exact typed binding | Wrong attempt, experiment, marker, title, cwd, ephemeral flag, response ID, reused thread ID, or immutable V14 snapshot/run-revision lineage is rejected atomically; one exact response binds once. Later ManagedRun revision advances preserve that immutable historical provenance. |
| Positive observations | Exact list/read/event/message facts append once with owned experiment, revision, thread, marker, source ID, digest, and database clock; malformed or cross-thread facts fail closed. |
| Lossy readback | Empty pages, pagination exhaustion, list omission, missing events, truncated history, stale caches, and incomplete readback persist no negative fact and cannot prove absence, completion, failure, or recreate authority. |
| Exact-command recovery | Same envelope/key replays stored bytes; changed envelope/key conflicts; aborts at receipt/domain/history completion roll back; no executing receipt commits. |
| ACL and hostile catalog | Runtime has only the four V15 command entrypoints plus required enum usage; PUBLIC, direct table writes, helpers, trigger bypass, hostile search path, overloads, default ACL drift, and dump/restore authority drift fail closed. |
| Production isolation | Reverse dependency inspection proves no production runtime or application reaches a V15 experiment execution root; Codex remains a typed fact adapter and dispatch remains disabled. |

#### XY-1367 V22 deferred acceptance matrix

This matrix is preserved as historical V22 acceptance context. XY-1368 executed its acceptance
against one exact staged V22 candidate after XY-1367 deferred executable validation. It does not
define a current command or the future package-defined V1-V23 retained-title task. The immutable
[XY-1368 freeze](xy-1368-retained-title-freeze.md) is historical evidence only; future delivery
uses the [private-artifact package](private-artifact/operations-delivery.md).

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Exact nullable-name start | The durable binding retains exact numeric request and raw response IDs, exact-frame SHA-256 digests, exact thread ID, cwd, marker, `ephemeral=false`, and nullable returned name. The pinned build accepts only raw null. A title read later cannot substitute for a start response field. |
| Creation crash and replay | A fresh creation fence authorizes one start. Fence replay never authorizes another start. An exact same-experiment and same-attempt durable binding resumes with only its exact ID. Missing binding after response loss remains terminally ambiguous. Search, list, adoption, retry, and inferred absence remain unavailable. |
| Title effect fence | Only a freshly committed title fence authorizes one exact `thread/name/set` request. The fence stores fixed request ID 4, the prepared title, and the exact-frame digest. Replay, response loss, method rejection, or any mismatch never authorizes another set. Database command replay retains the same derived key and byte-equivalent envelope. External RPC transport never retries. |
| Exact-ID retained-title attestation | After fence replay or a lost set response, one bounded exact-ID read can attest an already-correct title. Attestation requires the prepared title, marker, cwd, and thread ID plus exact read request and raw response IDs and digests. Missing, null, changed, or cross-thread facts remain ambiguous. |
| Observation and continuation gate | Only observations linked to an immutable retained-title attestation qualify as title evidence. V17 same-thread completeness requires that mapping and exact attestation lineage. Historical V15 rows remain readable but cannot satisfy the V22 title gate. |
| ACL and catalog closure | Runtime cannot execute the obsolete V15 title-in-start binder or unattested observation command. It can execute only the V22 bind, exact start readback, title fence, attestation, and attested observation additions. PUBLIC, direct DML, overload, trigger, owner, dependency, default-ACL, and restore drift fail closed across 77 relations, 161 functions, 67 safety functions, 142 triggers, and the V1-V22 ledger. |
| Inert runner reachability | The manual Rust runner requires its explicit Cargo feature and binary. Its only effect flow is prepare, creation fence, start, bind, title fence, name set, exact-ID read, attestation, and positive observation. `decodexd`, protocol handlers, scheduler, routing orchestration, production dispatch, and default features cannot reach it. It exposes no turn, list, search, archive, retry, adoption, account switch, plugin, or UI operation. |

#### XY-1359 deferred acceptance matrix

This source-only matrix is deferred to the XY-1364 unified frozen-core gate. It binds the V16
source, migration, configured-authority inventory, schema manifest, and authority digests to one
exact tree; no case may dispatch work or enable a production consumer.

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Deterministic order | Policy order and canonical account identity produce the same selected result across caller reorder, map order, pagination, and timing variation; caller omission, substitution, duplication, or extra candidates cannot alter the PostgreSQL universe. |
| Sticky eligibility | A sticky account wins only with the same current policy, identity/revision, capability, compatibility, blocker, and exact quota evidence required of every candidate; stale, disabled, auth-failed, incompatible, or depleted sticky accounts cannot bypass blockers. |
| Duration and precision | 300-minute and 10080-minute facts remain distinct; missing/unsupported duration, unknown/low confidence, malformed raw provenance, non-microsecond precision, and any would-round or would-truncate timestamp fail closed. Exact raw observed/reset text, source identity, evidence revision, and UTC Unix microseconds round-trip unchanged. |
| Selected exclusions | Every depleted predecessor ahead of the selected member has one normalized account/window exclusion tied to its immutable snapshot fact, observation revision, exact raw timestamps, source, precision, and deterministic `usage_depleted` reason; unrelated blockers are retained as references. |
| Waiting versus blocked | All otherwise eligible accounts depleted produces only `waiting_usage`, complete per-account/per-window exclusions, per-account maximum readiness, and the exact minimum of those readiness instants. Mixed depletion with unknown, incompatible, disabled, auth-failed, missing-duration, stale, or precision-incompatible evidence produces `no_route`, never a wake-ready decision. |
| Exact command replay | Same key and envelope replays byte-identical decision/evidence readback; changed operation or envelope conflicts; malformed input is a stable typed rejection; abort, lost response, deadlock, serialization failure, and restart never commit a partial decision, executing receipt, or duplicate effect. |
| Concurrent authority | Policy, snapshot, account, RoleProfile, capability, compatibility, blocker, quota, or ManagedRun changes before or during resolution either serialize against the complete lock boundary or return a typed stale/concurrent rejection; no mixed-universe decision commits. |
| Immutability and completeness | Decision, member, quota, capability, blocker, and exclusion rows commit together, are append-only after commit, retain the exact immutable V14 snapshot/run-revision lineage across later ManagedRun advances, and match strict Rust readback plus the pure kernel; missing, reordered, extra, cross-linked, or malformed fields fail closed. |
| ACL and hostile catalog | Runtime has only the V16 command entrypoint; PUBLIC, direct table writes, private helpers, trigger bypass, hostile search path, overload/default-ACL drift, ownership drift, and dump/restore catalog drift fail closed. Regenerated schema/configured-authority digests match the integrated frozen tree. |
| Production isolation | Reverse dependency inspection proves only the disabled XY-1361 runtime orchestration invokes exactly one V16 decision per request; no protocol, CLI, daemon, application, scheduler, Codex, credential, or UI production root constructs it. Selected alone may consume one exact V17 plan, while `waiting_usage` yields inert handoff facts for the separately uncomposed V18 wake authority. |

#### XY-1360 deferred acceptance matrix

This source-only matrix is deferred to the XY-1364 unified frozen-core gate. It binds V10, V12,
V15, V16, V17, V21, the strict Rust adapter, schema/configured-authority inventories, and
regenerated digests to one exact integrated tree; no case may dispatch work or enable a production
consumer.

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Decision consumption | Only one persisted `selected` V16 decision identity plus its exact immutable ManagedRun revision lineage is accepted; waiting/no-route, missing, stale, cross-run, substituted, or already-consumed decisions fail closed. Caller candidates, policy, exclusions, selection, evidence, and account facts cannot alter the database-derived lineage, and later ManagedRun advances do not rewrite or orphan the persisted plan. |
| Same-thread evidence | Exact selected account/revision, build, RoleProfile and source RuntimeSession identity, bound thread, V14 schema/capability profile, and a fresh V22-attested positive thread-read observation permit exactly one same-thread plan. Unknown, unattested, stale, future, negative, mismatched, incomplete, duplicate-experiment, unsupported, noncanonical, or lossy absence evidence cannot authorize same-thread continuation. |
| Atomic fallback | Crash before/after blob publication, exact-receipt reservation, source staging, Context-Pack seal, account snapshot, RuntimeSession, plan, activity, outbox, receipt completion, commit, or response loss leaves either no durable fallback state or one complete linked Context Pack + RuntimeSession + plan. No Context Pack-only, RuntimeSession-only, two-session, or V10/V16 two-command orphan is possible. |
| Cross-domain event references | Canonical HistoryItem and other non-RuntimeSession activity/outbox may carry scalar `runtime_session_id`, `profile_snapshot_id`, or `account_snapshot_id` provenance. RuntimeSession aggregate/event/kind markers, complete RuntimeSession/profile/account snapshot objects under any wrapper, and outbox links to activity carrying those shapes remain migration-owner-only; direct forgery and immutable-authority updates fail, while delivery-only outbox updates remain permitted. A neutral array whose incomplete object elements only collectively contain a complete shape remains allowed; one genuinely complete snapshot object nested in an array remains reserved. |
| Replay and concurrency | Same key replays exact bytes; a second key with the identical request reads the one stored plan; changed requests conflict or reject. Concurrent decision consumers, Context-Pack revisions, fallback identities, Conversation closure, ManagedRun revision change, and blob reclamation serialize or fail closed without duplicate state. |
| ManagedRun safety | Conversation and ManagedRun identities remain unchanged. Guarded/closed barrier revision and submitted-turn receipt count are snapshotted; `replay_permitted=false` and `dispatch_enabled=false` remain immutable for no-receipt, stale-receipt, possible-side-effect, unknown-side-effect, diverged, and reconciled fixtures. No turn, tool, repository, worktree, Git, or artifact effect is replayed. |
| Context-Pack hostile input | Canonical binary header, digest, manifest digest, source order, pinned source, disposition, represented-byte digest, bounds, credential-negative identities, Artifact revision/blob provenance, and inline/offloaded shape round-trip through strict readback. Truncated, reordered, forged, cross-Conversation, credential-shaped, oversized, malformed, and hash/length-conflicting inputs fail closed. |
| ACL and catalogs | PUBLIC, direct plan DML, helper execution, activity/outbox lineage forgery, trigger bypass, hostile `search_path`, overload/default-ACL drift, ownership drift, restore drift, and surplus runtime privileges fail closed. The exact V17 function, relation, enum, constraint, trigger, dependency, migration, schema, and configured-authority inventories and regenerated digests match. |
| End to end and isolation | One exact selected decision yields either one same-thread plan or one atomically verified fallback and survives restart readback byte-for-byte. Reverse dependency inspection proves only the disabled XY-1361 runtime orchestration reaches V17 and only after one selected V16 decision; no protocol, daemon, CLI, application, scheduler, credential, Codex, UI, or production composition root constructs it. |

#### XY-1362 V18/V19 deferred acceptance matrix

This source-only matrix is deferred to the XY-1364 unified frozen-core gate. It binds V10-V21, the
strict Rust adapter, migration and configured-authority inventories, and regenerated digests to one
exact integrated tree. It must not enable or compose a production scheduler.

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Exact registration and hostile input | Only one persisted V16 `waiting_usage` identity and its exact ManagedRun revision are accepted. Database-derived `earliest_ready_at` round-trips at exact microseconds. Caller timestamps, candidates, quota facts, eligibility, exclusions, accounts, policy bodies, and replacement decisions are absent from the API; missing, selected/no-route, forged, cross-run, malformed, or stale lineage fails closed. |
| Ledger-first registration and operation identity | One registered transition and one derived head bind the exact decision/run revision. Same-key replay returns stored receipt bytes; the same operation under a new key returns only its immutable registration result after canonical request equality. Retrying registration after later claim/fire cannot acquire later head state. A new operation ID targeting the registered decision, OR/non-strict identity lookup, conflicting reuse, concurrent registrar, or same-run competing decision rejects without aliasing or orphan state. |
| Deterministic time authority and fairness | Owner-only explicit time proves before/equal/after-ready behavior, including exact-ready claim, equal readiness, microsecond boundaries, and reordered callers while preserving order by earliest-ready instant, registration time, and wake identity. Explicit values outside `0..=253402300739999999`, infinity, registration before locked decision/run authority, and successor time before the locked tip reject without committed mutation. Typed-`NULL` runtime wrappers retain PostgreSQL-authored post-lock sampling and provide no injectable time. Independent account quotas are never pooled, merged, summed, averaged, or caller-ranked. |
| Transition chain and projection | Every accepted operation appends one immutable transition with the exact predecessor revision/tip and complete result, then atomically advances the head to that exact tip. Forged/skipped predecessors, direct head mutation, projection/ledger inequality, duplicate operation IDs, history rewrites, post-terminal successors, partial activity/outbox clusters, and mutable-head historical readback fail closed. |
| Lease, crash, and restart | Crash or rollback before/after transition insertion, head advance, activity/outbox insertion, receipt completion, commit, or response loss yields complete cluster absence or one complete command, never a partial cluster. The fixed literal 60-second lease excludes concurrent holders; pre-expiry reclaim rejects, exact expiry appends a reclaim with a new fence, and the old fence and stale tip reject. Strict restart/readback preserves both lease transitions without rewriting history. |
| Replay, concurrency, and exactly once | Same command key/envelope replays byte-identical bytes; changed envelopes conflict. Cross-key operation replay at a different valid explicit time, including after head movement, verifies the canonical domain request and returns only immutable result bytes. Duplicate fire, cancellation/fire, expiry/fire, and concurrent-holder schedules have the legal outcome multiset with at most one fired transition/request/effect. Stale fences and stale tips reject; no executing receipt or half-written transition/head/activity/outbox cluster commits. |
| Fire, cancellation, and stale lineage | A valid unexpired leased tip fires once. Explicit cancellation and every ManagedRun, policy, or decision staleness case append the appropriate cause-bound terminal transition and advance the exact head before delivery. Terminal transitions cannot have successors or return to pending/leased, and a stale expected revision/tip or lost lease fence cannot mutate the head. |
| Fresh resolution only | Fired readback contains one new routing-resolution request identity with `fresh_routing_resolution_only=true`, `prior_decision_reusable=false`, and `production_enabled=false`. Old member order, eligibility universe, quota/capability evidence, exclusions, selected account, V16 decision result, credential, continuation, dispatch, or retry authority cannot be reconstructed or reused from the effect. |
| ACL, search path, and catalogs | PUBLIC, direct transition/head DML, runtime execution of any V19 internal, private replay/helper execution, inherited or `SET ROLE` time injection, forged predecessor or decision/run/policy lineage, activity/outbox namespace forgery, trigger bypass, hostile `search_path`, overload/default-argument/default-ACL drift, ownership or grant-option drift, relation/enum/constraint/index/function-dependency drift, dump/restore drift, and surplus runtime privileges fail closed. The four exact internal and four exact wrapper bodies/metadata/settings/ACLs, unchanged 51-function runtime allowlist, 73 relations, 67 safety functions, 138 triggers, V1-V21 migration ledger, transition-bound strict readback, and regenerated schema/configured-authority digests match. |
| End to end and production isolation | Exact V16 wait -> registered transition/head -> claimed or reclaimed transition/head -> one fired transition with a fresh-resolution request survives restart and immutable strict readback; cancellation and every stale case emit no request. No command response is reconstructed from the head. Reverse dependency inspection proves no runtime, protocol, daemon, CLI, Codex, credential, continuation, dispatch, UI, or production composition root imports or invokes V18. XY-1304 remains the sole live gate. |

#### Integrated vNext core freeze deferred acceptance matrix

The `fd1e351` core freeze was reopened for the forward V19 time-authority repair. Forward-only V20
then canonicalized restore-unstable constraints, and V21 repairs only RuntimeSession event-reference
classification and its final inventories without changing the trusted command or trigger boundary.
XY-1367 reopened that tree only for forward V22. This matrix records the integrated source
candidate used by the historical XY-1368 V22 acceptance. It is not the future private-artifact
CORE-FREEZE, ACC, preparation, or unified-validation contract. Those future contracts exist only
in the [private-artifact package delivery module](private-artifact/operations-delivery.md).

| Boundary | Representative deferred acceptance cases |
| --- | --- |
| Scheduler timing and fairness | Before, equal, and after exact database-authored readiness; equal-ready deterministic ordering; clock-microsecond edges; bounded acquisition under a large independent-account inventory; no starvation introduced by caller order; and no pooling, merging, summing, averaging, or caller ranking of account quotas. |
| Crash and partial-transaction boundaries | V22 start and title fences obey their distinct replay rules. Lost start response without a durable binding remains terminal. Lost title-set response permits only exact-ID readback. Database commands retain exact envelope replay. Existing V16-V18 transaction clusters remain atomic. |
| Same-key and cross-key replay | Same protocol key and canonical envelope replays byte-identical stored bytes; changed envelopes conflict. A V17 decision cannot be consumed twice across keys. A V18 operation under a new key verifies canonical request equality and returns only its immutable transition result, never later mutable-head state. |
| Concurrent lifecycle operations | Concurrent register/register, claim/claim, claim/reclaim, reclaim/reclaim, fire/fire, cancel/cancel, fire/cancel, expiry/fire, and supersede races serialize to one legal append-only chain and exact head tip. Same-run competing V16 decisions cannot alias a wake, and every losing stale tip, claim, fence, or operation identity fails closed. |
| Lease expiry and restart reclaim | A fixed database-authored lease excludes concurrent holders; pre-expiry reclaim and stale-token fire reject; exact expiry and process restart append one reclaimed transition with a new fence while preserving the prior claim and lease history unchanged. |
| Stale ManagedRun, decision, and policy lineage | Missing, cross-run, wrong-kind, replaced, or ambiguous V16 decisions; stale ManagedRun revision/lifecycle/wait reason/barrier state; divergence; changed policy head; and substituted requested policy/run provenance reject or append only the cause-bound V18 supersession allowed by the exact current tip. |
| Terminal fencing | Fired, cancelled, and superseded tips admit no successor, reclaim, refire, recancel, or return to pending/leased. Lost leases, old transitions, forged predecessors, skipped revisions, and mutable-head/ledger inequality cannot mutate authority or reconstruct success. |
| V17 barrier readback | Same-thread plans additionally require the exact V22 retained-title attestation and mapped observation. Same-thread and fallback plans round-trip the exact ManagedRun barrier facts. Stale or substituted values fail closed. Replay and dispatch remain structurally false. |
| V16/V17/V18 provenance | Every disabled-orchestration outcome retains the exact requested routing policy and ManagedRun provenance. Exactly one V16 decision exists per request; selected alone consumes its one exact V17 plan; waiting exposes only its decision, ManagedRun revision, and exact earliest-ready handoff; V18 registration derives all remaining decision/policy/run lineage from PostgreSQL. |
| ACL, search path, catalog, and hostile input | PUBLIC and runtime authority close exactly over the intended V16-V18 entrypoints and enum usage. Direct relation writes, private helpers, trigger bypass, overload/default-ACL/ownership drift, hostile `search_path`, malformed UUID/revision/time/JSON/digest input, forged lineage, relation/enum/constraint/index/dependency drift, and dump/restore drift fail closed. |
| Exact effect, activity, outbox, and operation readback | Every accepted V17/V18 command binds immutable response/effect bytes, digest source, exact activity sequence/event, outbox identity/effect key, aggregate revision, operation identity, and transition/plan lineage. Missing, extra, reordered, cross-linked, half-written, or head-reconstructed values fail strict readback. |
| Fresh-resolution behavior | One valid leased V18 fire emits exactly one opaque new routing-resolution request with no old candidate universe, selection, account, eligibility, quota/capability evidence, exclusion, credential, continuation, dispatch, or retry authority; old-decision reuse and production enablement remain structurally false. |
| Production isolation and protocol vocabulary | Reverse dependencies prove no production root reaches the V22 runner. The runner requires its manual feature and binary. Existing V16/V17 orchestration and V18 remain disabled and uncomposed. No new V1 command exists. |
| Regenerated schema and configured-authority digests | The canonical frozen derivation and final bound run agree on the immutable V1-V21 checksums plus V22, 77 relations, 161 functions, 67 safety functions, 142 triggers, exact source/metadata/signatures/ACLs, enums, constraints, indexes, dependencies, schema manifest, configured-authority inventory, and expected digests. |
| Representative end-to-end flow | One request persists exactly one V16 result. Selected produces exactly one strict-readback V17 same-thread or atomic fallback plan and remains inert. Waiting produces only handoff facts, then an independently invoked V18 register -> claim or expiry/reclaim -> fire chain yields one immutable fresh-resolution request; cancellation and stale-lineage paths yield none. Restart preserves all readback and no production effect executes. |


## Cutover gate

Cutover may occur only after replacement behavior has accepted tests, XY-1304 has passed
through explicit repository authority, XY-1300 has accepted the later whole-product E2E, fault,
UI, packaging, and cutover boundary, and the v0.2 inventory is frozen. The accepted procedure
stops v0.2, verifies the trusted tag/cold
backup, initializes empty PostgreSQL state, explicitly recreates selected Projects and
Automations, and starts only vNext. It imports no legacy execution history and enables no
dual authority. Removal of old Linear/SQLite/Goal/operator transport follows replacement
proof, not speculative deletion.

The repository-owned XY-1261 receipt is
[the v0.2 freeze receipt](../evidence/v0.2-freeze.md). A destructive-removal task must
verify its exact external readbacks and resolve every recorded stop condition first. In
particular, the receipt records that the legacy SQLite database was already absent before
the freeze; its retirement sentinel is not a database backup, and later work must not
silently treat that acceptance gap as restored evidence.

## Stop conditions

Stop the owning gate on any contradiction with the authority contract, any unproven
authority boundary, credentials entering ordinary PostgreSQL rows, a second mutation
path around `decodexd`, possible side-effect replay without reconciliation, unbounded UI
history loading, or attempted remote binding before security acceptance. Decision-level
falsifiers are listed in the owning decision and require explicit architecture revision.

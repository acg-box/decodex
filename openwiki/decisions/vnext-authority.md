# Decodex vNext Authority Decision

Status: accepted repository authority for vNext. The XY-1403 private-artifact
retirement takes effect only at the exact repository effective point in the
[retirement decision](../specs/private-artifact/decision.md#repository-effective-point).

Tracking issue: [XY-1260](https://linear.app/hack-ink/issue/XY-1260/promote-the-vnext-authority-contract-and-supersede-lane-authority-v2)

Source planning record: [Decodex vNext Design Baseline](https://linear.app/hack-ink/document/decodex-vnext-design-baseline-681f7d8ad284), accepted 2026-07-12 against repository snapshot `5546dcb3b2eb0a8aecf7d6a3b117d2605ea315b8`.

Authority amendment: Manager decision accepted 2026-07-13. The decision accepts the
conceptual XY-1262 foundation/live-enablement split proposed by merged PR #1098 at
`687605583817eca32cbdfb1107f3ee18d3106cea`, but only through this independently
reviewed normative amendment. The merged proposal was evidence, not authority by
itself. Repository authority remains normative; Linear is planning metadata.

XY-1271 storage-boundary amendment accepted 2026-07-14: retain the migration and
daemon-private runtime identities, and treat `decodexd`, that identity, and BlobStore access as one
trusted service boundary. PostgreSQL owns committed metadata/domain/receipt/activity/outbox state;
local CAS owns large bytes, which PostgreSQL alone cannot attest. Blob-backed commands use a
receipt-first, fenced, exact-response saga and dedicated session-level hash plus per-shard
coordination around create-only synchronized CAS publication. Arbitrary/manual use of the private
runtime credential is unsupported and equivalent to daemon compromise.

Readiness authority correction accepted 2026-07-16: Codex 0.144.2 and 0.144.4 expose
no stable passive complete account-owned plugin, skill, and MCP readiness receipt. Passive
inventory is therefore withdrawn from the accepted XY-1262 foundation evidence. Host files,
manifests, configuration, remote catalogs, process binding, and user declarations do not become
observed account readiness. The first-release doctor result remains typed `unknown` for plugins;
`plugin_unready` is inert reserved state. XY-1336 tracks the upstream receipt gap outside the
vNext critical path and neither closes nor blocks XY-1275.

XY-1423 account-lifecycle authority correction accepted for the vNext replacement
boundary: PostgreSQL owns credential-negative account product state, a narrow
HostCredentialStore owns only versioned secret bundles, and the `decodexd` Account
Service coordinates every account operation. Versioned `enabled` state is independent
from observed health and quota. Fixed selection, balanced selection, and complete account
order are deterministic versioned CAS controls. The immediate boundary is
MacDogfoodReady; final AccountLifecycleReady adds Linux, ambient Codex auth, full account
presentation, and later automatic routing obligations.

The continuously watched legacy account file and environment-only access-token
projection are retired. Normal Mac dogfood startup and installation cannot read either
one. A clean local cutover creates an empty current database and uses the ordinary
versioned account-import command once per local credential. After public readback
verifies every account and routing control, the operator deletes the temporary import
files and old account source. The product has no bulk migration, compatibility reader,
dual authority, or fallback. Shared normal `~/.codex` remains Codex configuration,
plugin, rollout, and thread visibility authority.

The exact protected Codex build must positively prove the
`account/chatgptAuthTokens/refresh` callback before readiness. The current exact
`codex-cli 0.146.0-alpha.9.2` profile handles the root refresh request and response through
the Account Service, subject to exact-image, generated-schema, and live callback
preflights. The existing V23 ProcessGeneration intent, manifest, fence, and readback bind
the initial account revision, canonical credential version/fingerprint, provider identity,
and callback profile; no new ledger is added. The normative details are in
[Account Lifecycle Authority](../specs/account-lifecycle-authority.md).

XY-1274 quota-authority amendment accepted 2026-07-16: quota storage accepts only exact
product-valid Unix microseconds, and no layer may round or truncate an ingress timestamp.
Canonical quota mutation identities move to their `/2` typed-integer documents. The V8
migration is an atomic zero-state boundary that locks every quota writer surface, rejects all
structurally classified pre-V8 quota evidence, and alters the proven-empty `quota_windows`
table in place. Populated V7 conversion and table drop/recreation are not compatibility
features. Recovery is whole disposable pre-release database recreation; XY-1302 retains
whole-ledger production-baseline and cutover authority. XY-1357 must retain and characterize the
natural raw upstream timestamp before live quota ingestion or routing, and a value that cannot be
converted exactly to UTC Unix microseconds keeps production routing disabled and reopens only the
ingress authority. The normative
details and proof gates live in the authority contract and gate manifest; the three rejected
old-boundary candidates remain historical provenance in the runtime proof.

XY-1272 PostgreSQL-authority reset accepted 2026-07-16: XY-1272 owns only configured-principal
and ACL manifest/readiness closure against landed V8. It owns no migration or Codex creation,
mapping, or reconciliation surface. Under the later XY-1345 reset, XY-1346 owns expected V9 and
re-bounded XY-1337 owns expected V10; XY-1358/V15 owns causal experiment creation
and positive-only observation authority; XY-1276 owns production Quick Task creation. Lossy or
paginated Codex evidence cannot authorize negative `Present`, `Complete`, or context-free `Absent`
authority. A future configured PostgreSQL role must atomically extend configuration, bootstrap,
manifest/readiness, and negative tests.

XY-1345 exact-command authority amendment accepted 2026-07-16: pure database commands use
operation-specific, command-complete `SECURITY DEFINER` entrypoints. PostgreSQL constructs and
consumes the complete typed request envelope; runtime supplies only the protocol-scoped
idempotency key and typed operation inputs. A separate `decodex.exact_command_receipts` relation is
keyed by `(protocol_version, idempotency_key)`, while operation identity remains inside the
envelope so cross-operation reuse conflicts. Runtime has no exact-receipt table privilege and no
private-helper or canonical activity/outbox mutation authority. An executing row is transaction
internal: a `DEFERRABLE INITIALLY DEFERRED` commit-time invariant rejects every incomplete commit,
and completed response bytes and effects are immutable and undeletable. Stable domain rejection,
idempotency conflict, and retryable infrastructure failure are distinct outcomes.

Normal execution is one exact command per top-level `READ COMMITTED` transaction. The command uses
a later read/lock statement after `ON CONFLICT DO NOTHING`; classified `40001` and `40P01` failures
retry the whole identical transaction. Request JSONB uses equality, includes every optional key
with explicit JSON null, receives enums/numerics through typed inputs, and preserves exact
PostgreSQL text/code-point semantics. Derived revisions, selected rows, generated identities,
timestamps, digests, snapshots, activity/outbox identities, and responses are effects. Effect and
response evidence comes from actual `RETURNING` rows and canonical audit identities.

Candidate 3 is superseded as implementation and remains hostile-test/design provenance only.
[XY-1345 evidence](../evidence/xy-1345-exact-command-authority.md) records the passing isolated
PostgreSQL 18 proof. The serial vertical order is XY-1345 authority/prototype, then XY-1346 exact
receipts plus immutable global RoleProfile bootstrap/update in V9, then re-bounded XY-1337
RuntimeSession V10. Legacy
`command_receipts` semantics remain unchanged for unrelated external or long-running sagas.

XY-1337 RuntimeSession amendment, implemented as expected V10: creation and transition are separate
command-complete `SECURITY DEFINER` operations using the unchanged V9 exact receipt. PostgreSQL
constructs creation identity from RuntimeSession ID, Conversation ID, role, complete non-secret
account snapshot identity/facts, nullable Codex thread ID, and initial state, then resolves exactly
one current immutable RoleProfile revision server-side. Transition identity contains only session
ID, expected revision, and target state. Both commands atomically store domain state, canonical
activity/outbox effects, and immutable response bytes; stable rejection completes in the same
transaction. V10 is a zero-state forward cutover, preserves the existing table identities, removes
runtime snapshot/session DML, fences RuntimeSession audit namespaces, and does not authorize Codex
creation, reconciliation, routing, scheduling, WorkItems, ManagedRuns, UI, or plugin readiness.
Forward-only V21 repairs that fence without changing the command or trigger boundary: a scalar
RuntimeSession/profile/account snapshot identity is provenance owned by the enclosing domain event,
not a RuntimeSession ownership claim. Aggregate/event/kind markers, complete RuntimeSession or
profile/account snapshot objects under any wrapper, and outbox links to activity carrying those
shapes remain reserved.

XY-1284 managed-repository authority reset was finalized by the accepted XY-1348
stage-two amendment on 2026-07-17. PostgreSQL is the durable authority for the current
repository projection, monotonic generation/tip, globally immutable operation
assignment, append-only authority and operation evidence, compare-and-swap, atomic
command completeness, and restart loads. Pure deciders and facts in `decodex-core` are
mechanism-neutral inputs to that authority; no standalone object, snapshot, caller
projection, or operation view is authoritative or can grant execution.

Every repository operation ID has one complete canonical descriptor across all
repositories and operation kinds. Exact descriptor equality resolves to
`ExistingExact(OperationView, NoDispatch)`; any difference is permanent
`OperationIdConflict`. A fresh affine execution receipt exists only after successful
COMMIT acknowledgement on the same adapter control path that prepared the new assignment.
Persistence, readback, exact repeat, restart, terminal state, and an unknown COMMIT
outcome never mint or reconstruct it. An unknown COMMIT outcome therefore authorizes no
external execution.

Allocate is PostgreSQL-only and uses strictly read-only repository evidence. `Register`,
`WorktreeReady`, and `Commit` are distinct durably fenced `PossiblyEffected` operations with
readback-only restart: no retry, replay, adoption, repair, or import is authorized.
`Register` requires exact reciprocal registration, `WorktreeReady` preserves the exact
head, and `Commit` advances exactly once from its authorized predecessor head to its exact
successor. Accepted XY-1354 descriptor-assisted, symlink-free persisted absolute-path
reacquisition and pinned Git 2.54 remain unchanged. Rejected candidate trees
`6e20e9b3cf1415cce9b399da173b0410cc4c80dc`,
`6979e3831da772fca3fe0f0e0b4699df642d3a65`, and
`e42212add13af3f702e0ec8966ce3d6a7b682d12` are superseded evidence only, not current
authority. The accepted V11 commit `33159d0cb2da7f86748f1a380def0927970a409a`
and V12 commit `a6bfb0aefc72f2a65d14fc3755b556f959ec2d4e` remain unchanged.

V13 is accepted on `main` as the sole XY-1349 managed-repository PostgreSQL authority
migration. The serial routing migration owners are fixed: XY-1356 solely owns V14 durable
routing-policy and complete candidate-set authority; XY-1358 solely owns V15 causal Codex
experiment authority; XY-1359 solely owns V16 atomic routing decisions; XY-1360 owns V17
continuation authority after source inspection proved durable atomic fallback state was required;
and XY-1362 owns V18's ledger-first `waiting_usage` wake authority. Migration allocation, the
embedded ledger, migration authority, schema/digest inventory, and aggregate migration evidence
remain one non-commutative singleton serial-writer domain.

XY-1355 live-routing authority reset accepted 2026-07-18 after three materially rejected
XY-1304 candidates: PostgreSQL owns one revisioned complete routing-authority snapshot.
Runtime and callers cannot supply authoritative policy order, candidate membership, sticky
identity, eligibility facts, or exclusions. The database-produced snapshot binds every
account-inventory member to an explicit disposition; canonical user-owned order and accepted
Policy revision; sticky affinity and the exact RuntimeSession revision from which it was
derived; account, RoleProfile, and Codex-build compatibility; exact account and evidence
revisions; and the required capabilities plus applicability for each member. An omitted or
unknown inventory member is a blocker, never silent absence or an eligible candidate.
The existing bounded inert `PolicySnapshot` remains accepted Policy-revision content only; it is
not a complete routing snapshot and cannot prove candidate completeness or provenance.

`decodex-core` remains a pure decision kernel over that complete database-produced value.
PostgreSQL persists the atomic decision and evidence linkage; runtime sequences only effects;
Codex supplies positive capability evidence and never routing authority. One app-server process
remains immutably bound to one account, credentials never switch in a live process, and separate
account quotas are never represented as a merged pool.

Account-owned readiness is evaluated only for capabilities explicitly required by the accepted
routing policy. Unknown never satisfies a required capability. When the accepted required-
capability set is empty, unknown plugin inventory is non-applicable rather than positive readiness
evidence. XY-1336 remains future passive-receipt tracking outside this routing chain. Host-owned
before/after receipts establish only no-mutation integrity.

Ingress retains the exact raw provider timestamp value. UTC Unix-microsecond construction must be
exact and rejects every rounding or truncation path. V14 through V16 remain precision-agnostic and
fail closed, allowing XY-1357 natural provider evidence to remain in the unified post-freeze gate.

Mac Slice 1 uses V14/V16 only for quota-aware initial selection. `fixed` considers one
target. `balanced` selects the first fully eligible account in canonical order. Recovery
is an explicit versioned enable/disable, mode, or order change followed by a new task.
Unknown or stale quota is ineligible, and all-depleted waits for explicit retry.

After V16, XY-1360 retains same-thread continuation and atomic Context-Pack fallback;
XY-1362 retains the `waiting_usage` wake lifecycle; and XY-1363 retains title-discovery
evidence. These are later obligations. XY-1304 owns only aggregate acceptance and a
separate reviewed enablement amendment for automatic cross-account same-thread fallback
and all-depleted wake. It does not block Quick Task, Project/Lead, ManagedRun, GPUI, or
first Mac dogfood. Ambiguous-turn replay and repository, worktree, Git, and artifact
reconciliation remain in ProviderAttempt and the accepted repository-effect authorities,
not routing.

The rejected dirty combined XY-1304/V14 candidate, its partial fourth repair, its caller-
authoritative request shape, Rust authorization wrapper used as provenance, global
`SupportedPositive` plugin requirement, combined experiment/routing schema, and sequential
exclusion -> RuntimeSession -> decision composition are superseded evidence only. They must not
be repaired, revived, or wholesale transplanted.

XY-1403 selects Option 1. At and after its exact repository effective point, the
private-artifact lane is retired from vNext. The
[private-artifact archive](../specs/private-artifact/README.md) preserves the former
package and all public receipt anchors as historical evidence. Its rule ledger,
inventories, source census, V22 snapshots, corpus index, semantic modules, delivery
edges, A0/A1/B/D0a/C/D phases, CORE-FREEZE, ACC, preparation, and unified validation
are historical and non-executable. They are not current authority, runtime inputs,
dependencies, or future-work inventory.

The [XY-1372 capability evidence](../evidence/xy-1372-private-artifact-capabilities.md)
also remains historical evidence. It does not authorize a runtime, gate, platform
requirement, or downstream experiment. XY-1373's former moving-core integration and
landing condition is historical and non-executable. Its later cancellation preserves
its history, comments, parent XY-1371, and `relatedTo` relations to XY-1374 and
XY-1371. Cancellation does not claim that the integration completed.

XY-1371 and the XY-1378-XY-1391 private-artifact execution graph are inactive
historical planning provenance. Repository authority already retired that program.
They cannot gate a delivery slice or restore a private-artifact authority layer.

Only the retained-title evidence transport changes. XY-1369 and XY-1370 keep their
bounded operator checks and commit reviewed public-safe attestations and digests as
canonical Git evidence. Raw errors, paths, account or role text, credentials,
provider data, raw schema, and unrestricted output stay out of Git and receipts.
XY-1363 consumes the exact accepted Git receipt identities and uses the accepted V22
one-shot title path. No new service, schema, storage system, runtime route, platform
layer, issue, compatibility path, or product Artifact is created.

The accepted Artifact/BlobStore boundary for ordinary content-addressed product
evidence does not change. The external empty-state cutover and user-owned
RoleProfile/RuntimeSession authority also remain intact. Later automatic fallback and
wake remain disabled until the separate reviewed XY-1304 enablement amendment.

Historical AR-CLOSE and pre-retirement package identities remain provenance. In
particular, keep signed C2 commit
`019f58a31b976056c000b73de3ec46b89284c6eb`, tree
`a56976663774b1e901e27fdf4c5276a7e9c84cb8`, package tree
`881a7d25801a4795a343d620164ed74a6dae136c`, and raw pre-retirement
`authority/package.manifest` SHA-256
`f88d5706b08e70170531dcda991d841c3b43543cf96a77500c0304f4a469753e`.
These identities prove bytes only. They do not prove historical semantic fidelity
or restore package authority.

The V1 trust boundary is one trusted single-host service. `decodexd` remains the sole
repository-effect owner. Its in-process repository executor is a correctness,
determinism, and admitted-authority-continuity boundary, not isolation from malicious
same-UID code. Project validation may supervise lifecycle, bound output and time, and
detect mutation, but it is not hostile-code confinement. A hostile same-UID project or
multi-tenant requirement would require a separate UID/sandbox authority plus a new
feasibility gate; it cannot be inferred from this design.

Authorized whole-cluster restore is inside the trusted PostgreSQL-administrator boundary
and may redefine current authority; V1 has no automatic full-cluster rollback detection.
The trusted single-daemon/same-UID boundary remains unchanged. XY-1349 solely owns V13
persistence, XY-1350 may proceed in parallel only against this accepted contract, and
XY-1351 owns the first shared saga path.

## Decision

Decodex vNext is a rebuild of the agent workspace, not an incremental extension of the
current Linear-lane and SQLite runtime. The normative product and runtime contract is
[the vNext authority contract](../specs/vnext-authority.md); its ordered proof and
implementation boundaries are in [the vNext gate manifest](../specs/vnext-gates.md).

The accepted planning baseline is provenance for this decision. Where the baseline and
these repository documents agree, these repository documents own implementation. A
concrete contradiction must stop the affected milestone for explicit resolution; later
workers must not silently reinterpret either source.

## Decision hierarchy

Within vNext work, authority descends in this order:

1. explicit user direction and checked-in project policy;
2. this decision, the vNext authority contract, and the vNext gate manifest,
   including the XY-1403 private-artifact retirement; the retired package is
   historical evidence only;
3. accepted project policies and versioned domain/protocol contracts created under
   those documents;
4. source, tests, migrations, and operational runbooks implementing an accepted gate;
5. OpenWiki navigation and current-runtime descriptions;
6. Linear plans and research as provenance or candidate changes.

Current source remains authority for current v0.2 behavior but cannot override the
accepted vNext target. Conversely, target documents do not claim that vNext behavior is
already implemented. Git/filesystem and GitHub remain the authorities for repository
content and provider readback respectively; PostgreSQL becomes product-state authority
only when its bootstrap/cutover gate is accepted.

## Supersession

Lane Authority v2 and its C1-C7 program are superseded and frozen. Its decision,
contract, effect registry, gate manifest, checkpoint ledger, fixtures, scripts, review
findings, and incident scenarios remain useful historical evidence. They must not be
continued, repaired, activated, or treated as vNext acceptance gates. In particular,
vNext rejects Lane Authority v2's SQLite authority, Linear issue/lane identity, local
Unix authority ledger, and compatibility migration target.

PR #1092 is likewise historical/incident evidence: freeze or close it, do not merge it,
and do not wholesale cherry-pick its implementation. Reuse a behavior only after the
vNext owner and a focused gate/test make that behavior authoritative.

## Accepted shape

- PostgreSQL owns Decodex product state; a transactional outbox and leases support
  commands and recovery. It is not event sourced.
- A shared normal `~/.codex` owns persistent Codex rollout/thread continuity and Codex
  UI visibility. Decodex maps only threads that it created.
- `decodexd` alone owns scheduling, app-server children, product mutations, repository
  side effects, and adapters. GPUI, the menubar app, CLI, and MCP use the same versioned
  application protocol and are not parallel authority paths.
- Decodex owns Project, stable Advisor/Lead, execution-scoped Task/Reviewer, Program,
  Objective, WorkItem, Conversation, RuntimeSession, ManagedRun, Automation,
  ContextRevision, AgentMessage, Artifact, RoleProfile, and their policies.
- One Project has exactly one stable Lead in V1. The Lead serializes decisions while
  Task and Reviewer execution may be parallel. Advisor is global and advisory only.
- A managed implementation Task owns its implement/review/repair/revalidate loop by
  spawning an independent read-only reviewer subagent. Lead/Manager owns dispatch,
  final acceptance, and merge; Quick Tasks are exempt, and missing/failed review is a
  typed ManagedRun wait and optional WorkItem block rather than reviewed success.
- A Project policy grants bounded unattended authority once; repository, tool, path,
  merge, budget, approval, parallelism, and quiet-period limits remain hard stops.
- Work lands through focused task branches/PRs directly to `main`; there is no long-lived
  vNext branch.

Delivery uses exactly three vertical slices:

1. Accounts, Quick Task, and minimal Accounts/Conversation/Health GPUI, with no normal
   legacy watcher, credential environment projection, helper, or `:8192` authority.
2. Minimal Project/Lead/global Advisor entry, bounded Context Revision, WorkItem,
   ManagedRun, the existing repository saga, Task-Reviewer result, explicit human
   acceptance, and Project/Work/Run GPUI.
3. One representative two-account self-hosting repository E2E across restart boundaries
   and one Mac package, including clean-startup proof against legacy account authority.

The dependency recommendation is `Slice 1 -> Slice 2 -> Slice 3`. Current GPUI opens a
real shell and window. Health is the only bounded live destination. Every other
destination remains a placeholder. The Quick Task and WorkItem contracts do not make
their shell destinations live. GPUI is not generally usable. Remaining Slice 1 UI work
is Accounts and Conversation. The slices replace the former component-first/global-gate
sequence.

## XY-1262 gate split

The XY-1262 foundation gate is accepted only for the evidence scope enumerated in the
[gate manifest](../specs/vnext-gates.md): shared-home and one-account-per-process
boundaries, creation-receipt ownership, negotiated app-server contracts, supported
exact-ID/list/read/archive operations, lossy-read/divergence policy, native run-local
collaboration normalization, process-scoped authentication/redaction, read-only
integrity evidence, and pure duration-typed quota policy. This acceptance does not
claim global Codex title search, live quota-driven routing, automatic continuation, or
release readiness.

The separate [XY-1262 automatic-routing gate](https://linear.app/hack-ink/issue/XY-1304)
remains failed and fail-closed for automatic cross-account same-thread fallback and
all-depleted wake. It is not a global gate for the three slices. Slice 1 can use eligible
quota-aware fixed or balanced initial selection and manual recovery after its own account,
callback, ProcessGeneration, and ProviderAttempt fences pass. Unknown or stale quota never
establishes eligibility. Automatic fallback and wake still require a later explicit
repository authority amendment.

## Rejected alternatives and falsifiers

Rejected V1 alternatives are enumerated normatively in the contract. The decision may
change only for evidence that cross-account continuation cannot preserve useful
continuity, GPUI cannot meet pinned build/package/accessibility/test gates, PostgreSQL
cannot be operated reliably on the intended host, app-server cannot provide required
ownership/read/list/collaboration behavior, or real dogfood proves one Lead cannot keep
decision latency within policy. Such evidence blocks or revises the owning gate; it does
not authorize a compatibility facade or silent fallback.

For the XY-1284 managed-repository boundary, the prioritized falsifiers in the
[gate manifest](../specs/vnext-gates.md#xy-1284-managed-repository-authority-gate) are
incorporated into this decision as decision-changing evidence. In fixed order, they are:
an architecture that cannot preserve admitted authority or separate its distinct state
owners; unrecoverable or ambiguous restart/effect states; a security/authority path that
does not fail closed or cannot disable repository-controlled execution; evidence that
cannot distinguish completion from stale, duplicate, rollback, lost, or ambiguous
outcomes; integrity that permits unreserved, drifted, undetected, or unreconciled effects;
and performance that cannot meet an explicit later host budget without weakening an
earlier guarantee. An earlier class cannot be traded for success in a later class. Any
such contradiction stops the affected replacement gate and returns to an explicit
architecture decision; it never authorizes a fourth patch under the rejected combined
boundary, a mechanism outside the accepted stage-two contract, or a silent fallback.
